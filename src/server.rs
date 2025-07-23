use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};

use bevy::prelude::*;
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use bevy_quinnet::server::{QuinnetServer, QuinnetServerPlugin, ServerEndpointConfiguration};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use serde::{Deserialize, Serialize};

// ====== Сообщения ======

#[derive(Component)]
struct LocalPlayer;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Stance {
    Standing,
    Crouching,
    Prone,
}

impl Default for Stance {
    fn default() -> Self {
        Stance::Standing
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InputState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub rotation: f32,
    pub stance: Stance,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootEvent {
    pub shooter_id: u64,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub stance: Stance,
    pub hp: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldSnapshot {
    pub players: Vec<PlayerSnapshot>,
    pub server_time: f64,
}

// ====== Состояние сервера ======

#[derive(Default, Clone)]
struct PlayerState {
    pos: Vec2,
    rot: f32,
    stance: Stance,
    hp: i32,
}

#[derive(Resource, Default)]
struct PlayerStates(HashMap<u64, PlayerState>);

#[derive(Resource)]
struct ServerTickTimer(Timer);

#[derive(Resource)]
struct SnapshotHistory {
    // последние N снапшотов: (server_time, states)
    buf: VecDeque<(f64, HashMap<u64, PlayerState>)>,
    cap: usize,
}

impl Default for SnapshotHistory {
    fn default() -> Self {
        Self {
            buf: VecDeque::with_capacity(120),
            cap: 120, // ~2 секунды при 60 тиках
        }
    }
}

// ====== Константы ======

const TICK_DT: f32 = 0.015; // 64Hz
const MOVE_SPEED: f32 = 300.0;
const HITBOX_RADIUS: f32 = 20.0;
const MAX_RAY_LEN: f32 = 400.0;

// ====== Bevy App ======

fn main() {
    App::new()
        .insert_resource(PlayerStates::default())
        .insert_resource(ServerTickTimer(Timer::from_seconds(TICK_DT, TimerMode::Repeating)))
        .insert_resource(SnapshotHistory::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_systems(Startup, start_server)
        .add_systems(Update, (process_inputs_and_shots, server_tick))
        .run();
}

// ====== Startup ======

fn start_server(mut server: ResMut<QuinnetServer>) {
    let server_ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let server_port = 6000;

    let endpoint_config = ServerEndpointConfiguration::from_ip(server_ip, server_port);
    let cert_mode = CertificateRetrievalMode::GenerateSelfSigned {
        server_hostname: "localhost".to_string(),
    };

    let channels_config = ChannelsConfiguration::from_types(vec![
        // 0 — Input/Shoot (client -> server)
        ChannelKind::OrderedReliable { max_frame_size: 16_000 },
        // 1 — Snapshots (server -> clients)
        ChannelKind::OrderedReliable { max_frame_size: 16_000 },
    ])
    .unwrap();

    server
        .start_endpoint(endpoint_config, cert_mode, channels_config)
        .unwrap();

    println!("✅ Server started on {}:{}", server_ip, server_port);
}

// ====== Update systems ======

/// Читаем входы и выстрелы, обновляем состояние игроков (позиции), считаем хиты
fn process_inputs_and_shots(
    mut server: ResMut<QuinnetServer>,
    mut states: ResMut<PlayerStates>,
    time: Res<Time>,
    mut history: ResMut<SnapshotHistory>,
) {
    let now = time.elapsed_secs_f64();
    let endpoint = server.endpoint_mut();

    for client_id in endpoint.clients() {
        // InputState
        while let Some((_chan, input)) = endpoint.try_receive_message_from::<InputState>(client_id) {
            let entry = states.0.entry(client_id).or_insert(PlayerState {
                pos: Vec2::ZERO,
                rot: 0.0,
                stance: Stance::Standing,
                hp: 100,
            });

            // движение
            let mut dir = Vec2::ZERO;
            if input.up {
                dir.y += 1.0;
            }
            if input.down {
                dir.y -= 1.0;
            }
            if input.left {
                dir.x -= 1.0;
            }
            if input.right {
                dir.x += 1.0;
            }
            dir = dir.normalize_or_zero();
            entry.pos += dir * MOVE_SPEED * TICK_DT;

            // поворот / стойка
            entry.rot = input.rotation;
            entry.stance = input.stance;
        }

        // ShootEvent
        while let Some((_chan, shoot)) = endpoint.try_receive_message_from::<ShootEvent>(client_id)
        {
            if let Some(hit_id) = check_hit_lag_comp(&history.buf, &states.0, &shoot) {
                // шанс попадания по стойке
                let chance = match states.0.get(&hit_id).map(|p| &p.stance) {
                    Some(Stance::Standing) => 0.8,
                    Some(Stance::Crouching) => 0.5,
                    Some(Stance::Prone) => 0.2,
                    _ => 0.0,
                };
                if rand::random::<f32>() < chance {
                    if let Some(target) = states.0.get_mut(&hit_id) {
                        target.hp -= 20;
                        println!(
                            "💥 {} hit {} (HP: {})",
                            shoot.shooter_id, hit_id, target.hp
                        );
                    }
                } else {
                    println!("❌ {} missed {}", shoot.shooter_id, hit_id);
                }
            }
        }
    }

    // Сохраняем снапшот для lag compensation
    push_history(&mut history, now, &states.0);
}

/// Каждые 15 мс шлём снапшот всем клиентам
fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    states: Res<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let snapshot: WorldSnapshot = WorldSnapshot {
        players: states
            .0
            .iter()
            .map(|(&id, st)| PlayerSnapshot {
                id,
                x: st.pos.x,
                y: st.pos.y,
                rotation: st.rot,
                stance: st.stance.clone(),
                hp: st.hp,
            })
            .collect(),
        server_time: time.elapsed_secs_f64(),
    };

    let endpoint = server.endpoint_mut();
    if let Err(err) = endpoint.broadcast_message_on(1, snapshot) {
        eprintln!("❌ Broadcast failed: {:?}", err);
    }
}

// ====== Helpers ======

fn push_history(
    history: &mut ResMut<SnapshotHistory>,
    now: f64,
    states: &HashMap<u64, PlayerState>,
) {
    history.buf.push_back((now, states.clone()));
    if history.buf.len() > history.cap {
        history.buf.pop_front();
    }
}

fn check_hit_lag_comp(
    history: &VecDeque<(f64, HashMap<u64, PlayerState>)>,
    current: &HashMap<u64, PlayerState>,
    shoot: &ShootEvent,
) -> Option<u64> {
    // выбираем снапшот, ближайший по времени
    let states_at_shot: &HashMap<u64, PlayerState> = history
        .iter()
        .min_by(|a, b| {
            (a.0 - shoot.timestamp)
                .abs()
                .partial_cmp(&(b.0 - shoot.timestamp).abs())
                .unwrap()
        })
        .map(|(_, m)| m)
        .unwrap_or(current);

    let shooter = states_at_shot.get(&shoot.shooter_id)?;
    let shooter_pos = shooter.pos;
    let dir = shoot.dir.normalize_or_zero();

    for (&id, target) in states_at_shot.iter() {
        if id == shoot.shooter_id {
            continue;
        }
        let target_pos = target.pos;
        let to_target = target_pos - shooter_pos;
        let proj = to_target.project_onto(dir);
        if proj.length() <= MAX_RAY_LEN && to_target.distance(proj) <= HITBOX_RADIUS {
            return Some(id);
        }
    }
    None
}
