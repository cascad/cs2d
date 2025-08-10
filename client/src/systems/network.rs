use std::str::FromStr;

use crate::app_state::AppState;
use crate::components::{Corpse, GrenadeNet, LocalPlayer, PlayerMarker};
use crate::constants::{BULLET_SPEED, BULLET_TTL};
use crate::events::{
    GrenadeDetonatedEvent, GrenadeSpawnEvent, PlayerDamagedEvent, PlayerDied, PlayerLeftEvent,
};
use crate::menu::ConnectTimeout;
use crate::resources::grenades::{GrenadeStates, NetState};
use crate::resources::{
    ClientLatency, DeadPlayers, HpUiMap, LastKnownPos, MyPlayer, PendingInputsClient,
    SnapshotBuffer, SpawnedPlayers, TimeSync, UiFont, WallAabbCache,
};
use crate::systems::shoot::spawn_tracer;
use crate::systems::utils::{raycast_to_walls_cached, spawn_hp_ui, time_in_seconds};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_S2C, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{InputState, S2C};

#[derive(SystemParam)]
pub struct NetCtx<'w, 's> {
    pub commands: Commands<'w, 's>,

    pub buffer: ResMut<'w, SnapshotBuffer>,
    pub time_sync: ResMut<'w, TimeSync>,
    pub my: ResMut<'w, MyPlayer>,
    pub pending: ResMut<'w, PendingInputsClient>,
    pub spawned: ResMut<'w, SpawnedPlayers>,
    pub dead: ResMut<'w, DeadPlayers>,
    pub latency: ResMut<'w, ClientLatency>,
    pub hp_ui_map: ResMut<'w, HpUiMap>,

    pub q_local: Query<'w, 's, &'static mut Transform, With<LocalPlayer>>,
    pub q_marker: Query<'w, 's, (Entity, &'static PlayerMarker)>,

    pub font: Res<'w, UiFont>,

    // события
    pub ev_damage: EventWriter<'w, PlayerDamagedEvent>,
    pub ev_died: EventWriter<'w, PlayerDied>,
    pub ev_left: EventWriter<'w, PlayerLeftEvent>,
    pub ev_grenade_spawn: EventWriter<'w, GrenadeSpawnEvent>,
    pub ev_grenade_detonated: EventWriter<'w, GrenadeDetonatedEvent>,

    // прочее
    pub grenade_states: ResMut<'w, GrenadeStates>,
    pub wall_cache: Res<'w, WallAabbCache>,
    pub last_pos: Option<ResMut<'w, LastKnownPos>>,
    pub app_state: Res<'w, State<AppState>>,
    pub next_state: ResMut<'w, NextState<AppState>>,
}

pub fn receive_server_messages(mut client: ResMut<QuinnetClient>, mut net: NetCtx) {
    // если дефолтного соединения нет — выходим
    let Some(conn) = client.get_connection_mut() else {
        return;
    };

    while let Some((chan, msg)) = conn.try_receive_message::<S2C>() {
        if chan != CH_S2C {
            continue;
        }
        match msg {
            // ===================================================
            // 1) СНАПШОТ
            // ===================================================
            S2C::Snapshot(snap) => {
                let now_client = time_in_seconds();

                // time sync один раз на пустом буфере
                if net.buffer.snapshots.is_empty() {
                    net.time_sync.offset = now_client - snap.server_time;
                    info!("[Network] time sync offset = {:.3}", net.time_sync.offset);
                }

                // reconciliation локального игрока
                if let Ok(mut t) = net.q_local.single_mut() {
                    if let Some(ack) = snap.last_input_seq.get(&net.my.id) {
                        if let Some(ps) = snap.players.iter().find(|p| p.id == net.my.id) {
                            t.translation = Vec3::new(ps.x, ps.y, t.translation.z);
                            t.rotation = Quat::from_rotation_z(ps.rotation);
                            while let Some(front) = net.pending.0.front() {
                                if front.seq <= *ack {
                                    net.pending.0.pop_front();
                                } else {
                                    break;
                                }
                            }
                            for inp in net.pending.0.iter() {
                                simulate_input(&mut *t, inp);
                            }
                        }
                    }
                }

                // спавним новых из снапшота, обновляем HP-UI и last_pos
                for p in &snap.players {
                    let id = p.id;

                    if net.dead.0.contains(&id) {
                        continue;
                    }

                    if net.spawned.0.insert(p.id) {
                        let label = String::from_str("snapshot").unwrap();
                        spawn_player(&mut net.commands, &net.my, id, p.x, p.y, p.rotation, label);
                    }

                    if !net.hp_ui_map.0.contains_key(&p.id) {
                        let entity =
                            spawn_hp_ui(&mut net.commands, p.id, p.hp as u32, net.font.0.clone());
                        net.hp_ui_map.0.insert(p.id, entity);
                    }

                    if let Some(last_pos) = net.last_pos.as_deref_mut() {
                        last_pos.0.insert(p.id, (Vec2::new(p.x, p.y), p.rotation));
                    }
                }

                // переход из Connecting в InGame по первому снапу
                if matches!(net.app_state.get(), AppState::Connecting) {
                    net.commands.remove_resource::<ConnectTimeout>();
                    net.next_state.set(AppState::InGame);
                }

                // буфер для интерполяции
                net.buffer.snapshots.push_back(snap);
                while net.buffer.snapshots.len() > 120 {
                    net.buffer.snapshots.pop_front();
                }
            }

            // ===================================================
            // 2) СТРЕЛЬБА
            // ===================================================
            S2C::ShootFx(fx) => {
                info!("💥 [Client] got FX from {} at {:?}", fx.shooter_id, fx.from);

                let max_dist = BULLET_SPEED * BULLET_TTL;
                let dir = fx.dir.normalize_or_zero();
                let hit_dist = raycast_to_walls_cached(fx.from, dir, max_dist, &net.wall_cache.0);

                if hit_dist > 0.5 {
                    let ttl = hit_dist / BULLET_SPEED;
                    spawn_tracer(&mut net.commands, fx.from, dir, ttl);
                }
            }

            // ===================================================
            // 3) СПАВН / РЕСПАВН
            // ===================================================
            S2C::PlayerConnected { id, x, y } | S2C::PlayerRespawn { id, x, y } => {
                net.dead.0.remove(&id);
                net.buffer.snapshots.clear();

                if net.spawned.0.remove(&id) {
                    for (ent, marker) in net.q_marker.iter() {
                        if marker.0 == id {
                            net.commands.entity(ent).despawn();
                            break;
                        }
                    }
                }

                let rotation = 0.0;
                let label = String::from_str("new/respawn").unwrap();
                spawn_player(&mut net.commands, &net.my, id, x, y, rotation, label);
                net.spawned.0.insert(id);

                if let Some(last_pos) = net.last_pos.as_deref_mut() {
                    last_pos.0.insert(id, (Vec2::new(x, y), rotation));
                }
            }

            // ===================================================
            // 4) ИГРОК ВЫШЕЛ
            // ===================================================
            S2C::PlayerLeft(left_id) => {
                net.dead.0.remove(&left_id);

                if let Some((entity, _)) =
                    net.q_marker.iter().find(|(_, marker)| marker.0 == left_id)
                {
                    net.commands.entity(entity).despawn();
                    net.spawned.0.remove(&left_id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", left_id);
                }

                net.ev_left.write(PlayerLeftEvent(left_id));
            }

            // дубль, если прилетел другой тип уведомления
            S2C::PlayerDisconnected { id } => {
                net.dead.0.remove(&id);

                if let Some((entity, _)) = net.q_marker.iter().find(|(_, marker)| marker.0 == id) {
                    net.commands.entity(entity).despawn();
                    net.spawned.0.remove(&id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", id);
                }

                net.ev_left.write(PlayerLeftEvent(id));
            }

            // ===================================================
            // 5) PONG
            // ===================================================
            S2C::Pong {
                client_time,
                server_time,
            } => {
                let now = time_in_seconds();
                let rtt = now - client_time;
                let one_way = (rtt - (now - server_time)) * 0.5;
                net.latency.rtt = rtt;
                net.latency.offset = server_time - (client_time + one_way);
            }

            // ===================================================
            // 6) ДАМАГ
            // ===================================================
            S2C::PlayerDamaged { id, new_hp, damage } => {
                net.ev_damage
                    .write(PlayerDamagedEvent { id, new_hp, damage });
            }

            // ===================================================
            // 7) ГРАНАТЫ
            // ===================================================
            S2C::GrenadeSpawn(ev) => {
                let printable_ev = ev.clone();
                net.ev_grenade_spawn.write(GrenadeSpawnEvent(ev));
                info!("💣 GrenadeSpawn {}", printable_ev.id);
            }

            S2C::GrenadeSync { id, pos, vel, ts } => {
                // info!("grenades sync: {:?}", pos);
                let e = net.grenade_states.0.entry(id).or_default();
                *e = NetState {
                    pos,
                    vel,
                    ts,
                    has: true,
                };
            }

            S2C::GrenadeDetonated { id, pos } => {
                net.ev_grenade_detonated
                    .write(GrenadeDetonatedEvent { id, pos });
            }

            // ===================================================
            // 8) СМЕРТЬ
            // ===================================================
            S2C::PlayerDied { victim, killer } => {
                info!("[Client]   PlayerDied victim={}", victim);

                if let Some(last_pos) = net.last_pos.as_ref() {
                    if let Some((pos, rot)) = last_pos.0.get(&victim).cloned() {
                        net.commands.spawn((
                            Sprite {
                                color: Color::srgba(0.6, 0.15, 0.15, 1.0),
                                custom_size: Some(Vec2::splat(PLAYER_SIZE)),
                                ..default()
                            },
                            Transform::from_xyz(pos.x, pos.y, -0.1)
                                .with_rotation(Quat::from_rotation_z(rot)),
                            GlobalTransform::default(),
                            Corpse {
                                timer: Timer::from_seconds(8.0, TimerMode::Once),
                            },
                        ));
                    }
                }

                net.dead.0.insert(victim);

                if victim == net.my.id {
                    for (ent, _) in net.q_marker.iter().filter(|(_, m)| m.0 == victim) {
                        net.commands.entity(ent).despawn();
                        net.spawned.0.remove(&victim);
                    }
                    net.buffer.snapshots.clear();
                } else if let Some((ent, _)) = net.q_marker.iter().find(|(_, m)| m.0 == victim) {
                    net.commands.entity(ent).despawn();
                    net.spawned.0.remove(&victim);
                }

                net.ev_died.write(PlayerDied { victim, killer });
                info!("💀 Игрок {} погиб ({:?})", victim, killer);
            }
        }
    }
}

fn simulate_input(t: &mut Transform, inp: &InputState) {
    let mut dir = Vec2::ZERO;
    if inp.up {
        dir.y += 1.0;
    }
    if inp.down {
        dir.y -= 1.0;
    }
    if inp.left {
        dir.x -= 1.0;
    }
    if inp.right {
        dir.x += 1.0;
    }
    dir = dir.normalize_or_zero();
    t.translation += (dir * MOVE_SPEED * TICK_DT).extend(0.0);
    t.rotation = Quat::from_rotation_z(inp.rotation);
}

// Утилита для единообразного создания сущности игрока.
fn spawn_player(
    commands: &mut Commands,
    me: &ResMut<MyPlayer>,
    id: u64,
    x: f32,
    y: f32,
    rot: f32,
    from: String,
) -> Entity {
    let tf = Transform::from_xyz(x, y, 0.0).with_rotation(Quat::from_rotation_z(rot));
    let is_local = id == me.id;

    let entity = commands
        .spawn((
            Sprite {
                color: if is_local {
                    Color::srgba(0.0, 1.0, 0.0, 1.0) // зелёный
                } else {
                    Color::srgba(0.0, 0.0, 1.0, 1.0) // синий
                },
                custom_size: Some(Vec2::splat(PLAYER_SIZE)),
                ..default()
            },
            tf,
            GlobalTransform::default(),
            PlayerMarker(id),
            Name::new(format!(
                "Player[{}] {}",
                if is_local { "LOCAL" } else { "REMOTE" },
                id
            )),
        ))
        .id();

    if is_local {
        // Маркер, что это локальный игрок
        commands.entity(entity).insert(LocalPlayer);
        // Компонент для плагина камеры (только локальному), только если
        // надо ехать за игроком
        // commands.entity(entity).insert(Velocity(Vec2::ZERO));
        info!("[Client]{from} spawn LOCAL {}", id);
    } else {
        info!("[Client][{from}] spawn REMOTE {}", id);
    }

    entity
}

/// Применяем сетевое состояние к Transform гранат.
/// Между снапшотами — лёгкая экстраполяция pos += vel * (now - ts)
pub fn apply_grenade_net(
    states: Res<GrenadeStates>,
    time_sync: Res<TimeSync>,
    mut q: Query<(&GrenadeNet, &mut Transform)>,
) {
    let now_server = time_in_seconds() - time_sync.offset; // серверные секунды
    for (net, mut tf) in q.iter_mut() {
        // info!("apply id={} pos={:?}", net.id, tf.translation.truncate());

        if let Some(s) = states.0.get(&net.id) {
            if !s.has {
                continue;
            }
            let mut dt = (now_server - s.ts) as f32;
            if !time_sync.offset.is_finite() {
                dt = 0.0;
            }
            dt = dt.clamp(0.0, 0.25);
            let pos = s.pos + s.vel * dt;
            tf.translation.x = pos.x;
            tf.translation.y = pos.y;
        }
    }
}
