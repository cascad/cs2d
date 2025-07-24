use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};

use bevy::prelude::*;
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use bevy_quinnet::server::{QuinnetServer, QuinnetServerPlugin, ServerEndpointConfiguration};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use serde::{Deserialize, Serialize};
use ctrlc;

// ===== Channels =====
const CH_C2S: u8 = 0; // client → server (Input + Shoot)
const CH_S2C: u8 = 1; // server → clients (Snapshot + FX)

// ===== Client→Server enum =====
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum C2S {
    Input(InputState),
    Shoot(ShootEvent),
}

// ===== Server→Client enum =====
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum S2C {
    Snapshot(WorldSnapshot),
    ShootFx(ShootFx),
}

// ===== Core Message Types =====
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InputState {
    pub seq: u32,
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
pub struct ShootFx {
    pub shooter_id: u64,
    pub from: Vec2,
    pub dir: Vec2,
    pub timestamp: f64,
}

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
    pub last_input_seq: HashMap<u64, u32>,
}

// ===== Server State =====
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
    buf: VecDeque<(f64, HashMap<u64, PlayerState>)>,
    cap: usize,
}
impl Default for SnapshotHistory {
    fn default() -> Self {
        Self {
            buf: VecDeque::with_capacity(120),
            cap: 120,
        }
    }
}

#[derive(Resource, Default)]
struct PendingInputs(HashMap<u64, VecDeque<InputState>>);

#[derive(Resource, Default)]
struct AppliedSeqs(HashMap<u64, u32>);

// ===== Constants =====
const TICK_DT: f32 = 0.015; // 64Hz
const MOVE_SPEED: f32 = 300.0;
const HITBOX_RADIUS: f32 = 20.0;
const MAX_RAY_LEN: f32 = 400.0;

// ===== Bevy App =====
fn main() {
    // Ctrl‑C → graceful exit
    ctrlc::set_handler(move || {
        println!("⚡ Server shutting down");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl‑C handler");

    App::new()
        .insert_resource(PlayerStates::default())
        .insert_resource(PendingInputs::default())
        .insert_resource(AppliedSeqs::default())
        .insert_resource(ServerTickTimer(Timer::from_seconds(TICK_DT, TimerMode::Repeating)))
        .insert_resource(SnapshotHistory::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_systems(Startup, start_server)
        .add_systems(Update, (process_c2s_messages, server_tick).chain())
        .run();
}

// ===== Startup =====
fn start_server(mut server: ResMut<QuinnetServer>) {
    let endpoint_config = ServerEndpointConfiguration::from_ip(IpAddr::V4(Ipv4Addr::new(127,0,0,1)), 6000);
    let cert_mode = CertificateRetrievalMode::GenerateSelfSigned { server_hostname: "localhost".into() };
    let channels = ChannelsConfiguration::from_types(vec![
        ChannelKind::OrderedReliable   { max_frame_size: 16_000 }, // CH_C2S
        ChannelKind::OrderedReliable   { max_frame_size: 16_000 }, // CH_S2C
    ]).unwrap();
    server.start_endpoint(endpoint_config, cert_mode, channels).unwrap();
    println!("✅ Server started on 127.0.0.1:6000");
}

// ===== Systems =====
/// Обрабатываем единый поток C2S-сообщений
fn process_c2s_messages(
    mut server: ResMut<QuinnetServer>,
    mut pending: ResMut<PendingInputs>,
    states: Res<PlayerStates>,
    mut history: ResMut<SnapshotHistory>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let endpoint = server.endpoint_mut();

    for client_id in endpoint.clients() {
        while let Some((chan, msg)) = endpoint.try_receive_message_from::<C2S>(client_id) {
            debug_assert_eq!(chan, CH_C2S);
            match msg {
                C2S::Input(input) => {
                    pending.0.entry(client_id).or_default().push_back(input);
                }
                C2S::Shoot(shoot) => {
                    println!("🔫 [Server] ShootEvent from {}: {:?}", client_id, shoot);
                    // lag‑comp hit‑check
                    if let Some(hit) = check_hit_lag_comp(&history.buf, &states.0, &shoot) {
                        println!("💥 [Server] hit target {}", hit);
                    }
                    // broadcast FX
                    if let Some(st) = states.0.get(&shoot.shooter_id) {
                        let fx = ShootFx {
                            shooter_id: shoot.shooter_id,
                            from: st.pos,
                            dir: shoot.dir,
                            timestamp: shoot.timestamp,
                        };
                        // println!("📤 [Server] broadcast ShootFx: {:?}", fx);
                        endpoint
                            .broadcast_message_on(CH_S2C, S2C::ShootFx(fx))
                            .unwrap();
                    }
                }
            }
        }
    }

    // сохраняем историю
    push_history(&mut history, now, &states.0);
}

/// Физический тик + рассылка снапшота
fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut server: ResMut<QuinnetServer>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    // 1) применяем входы
    for (&id, queue) in pending.0.iter_mut() {
        if let Some(last) = queue.back() {
            let st = states.0.entry(id).or_default();
            let mut dir = Vec2::ZERO;
            if last.up    { dir.y += 1.0; }
            if last.down  { dir.y -= 1.0; }
            if last.left  { dir.x -= 1.0; }
            if last.right { dir.x += 1.0; }
            st.pos += dir.normalize_or_zero() * MOVE_SPEED * TICK_DT;
            st.rot = last.rotation;
            st.stance = last.stance.clone();
            applied.0.insert(id, last.seq);
        }
        queue.clear();
    }
    // 2) билд & broadcast snapshot
    let snapshot = WorldSnapshot {
        players: states.0.iter().map(|(&id, st)| PlayerSnapshot {
            id,
            x: st.pos.x,
            y: st.pos.y,
            rotation: st.rot,
            stance: st.stance.clone(),
            hp: st.hp,
        }).collect(),
        server_time: time.elapsed_secs_f64(),
        last_input_seq: applied.0.clone(),
    };
    server.endpoint_mut().broadcast_message_on(CH_S2C, S2C::Snapshot(snapshot)).unwrap();
}

// ===== Helpers =====

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
    let states_at_shot = history.iter()
        .min_by(|a, b| (a.0 - shoot.timestamp).abs().partial_cmp(&(b.0 - shoot.timestamp).abs()).unwrap())
        .map(|(_, m)| m)
        .unwrap_or(current);

    let shooter = states_at_shot.get(&shoot.shooter_id)?;
    let shooter_pos = shooter.pos;
    let dir = shoot.dir.normalize_or_zero();

    for (&id, target) in states_at_shot.iter() {
        if id == shoot.shooter_id { continue; }
        let to_target = target.pos - shooter_pos;
        let proj = to_target.project_onto(dir);
        if proj.length() <= MAX_RAY_LEN && to_target.distance(proj) <= HITBOX_RADIUS {
            return Some(id);
        }
    }
    None
}
