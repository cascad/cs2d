use std::str::FromStr;

use crate::components::{Bullet, GrenadeNet, LocalPlayer, PlayerMarker};
use crate::constants::{BULLET_SPEED, BULLET_TTL};
use crate::events::{
    GrenadeDetonatedEvent, GrenadeSpawnEvent, PlayerDamagedEvent, PlayerDied, PlayerLeftEvent,
};
use crate::resources::grenades::{GrenadeStates, NetState};
use crate::resources::{
    ClientLatency, DeadPlayers, HpUiMap, MyPlayer, PendingInputsClient, SnapshotBuffer,
    SpawnedPlayers, TimeSync, UiFont, WallAabbCache,
};
use crate::systems::level::Wall;
use crate::systems::utils::{
    raycast_to_walls, raycast_to_walls_cached, spawn_hp_ui, time_in_seconds,
};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_S2C, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{InputState, S2C};

pub fn receive_server_messages(
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
    mut buffer: ResMut<SnapshotBuffer>,
    mut time_sync: ResMut<TimeSync>,
    mut my: ResMut<MyPlayer>,
    mut pending: ResMut<PendingInputsClient>,
    mut q_local: Query<&mut Transform, With<LocalPlayer>>,
    mut spawned: ResMut<SpawnedPlayers>,
    mut dead: ResMut<DeadPlayers>,
    // todo fix q and query
    q_marker: Query<(Entity, &PlayerMarker)>,
    mut latency: ResMut<ClientLatency>,
    // mut ev_damage: EventWriter<PlayerDamagedEvent>,
    mut hp_ui_map: ResMut<HpUiMap>,
    // mut ev_player_died: EventWriter<PlayerDied>,
    // mut ev_player_left: EventWriter<PlayerLeftEvent>,
    // mut grenade_spawn_events: EventWriter<GrenadeSpawnEvent>,
    font: Res<UiFont>,
    mut grenade_states: ResMut<GrenadeStates>,
    wall_cache: Res<WallAabbCache>,

    // 🔽 четыре EventWriter-a свернули в один параметр
    mut events: ParamSet<(
        EventWriter<PlayerDamagedEvent>,    // p0
        EventWriter<PlayerDied>,            // p1
        EventWriter<PlayerLeftEvent>,       // p2
        EventWriter<GrenadeSpawnEvent>,     // p3
        EventWriter<GrenadeDetonatedEvent>, // p4
    )>,
) {
    let conn = client.connection_mut();

    while let Some((chan, msg)) = conn.try_receive_message::<S2C>() {
        if chan != CH_S2C {
            continue;
        }
        // info!("[Client] ← got {:?} on channel {}", msg, chan);
        match msg {
            // ===================================================
            // 1) СНАПШОТ
            // ===================================================
            S2C::Snapshot(snap) => {
                let now_client = time_in_seconds();

                // 1) Time sync — один раз, на пустом буфере
                if buffer.snapshots.is_empty() {
                    time_sync.offset = now_client - snap.server_time;
                    info!("[Network] time sync offset = {:.3}", time_sync.offset);
                }

                // 2) Reconciliation локального игрока
                if let Ok(mut t) = q_local.single_mut() {
                    if let Some(ack) = snap.last_input_seq.get(&my.id) {
                        if let Some(ps) = snap.players.iter().find(|p| p.id == my.id) {
                            // сброс позы до серверной
                            t.translation = Vec3::new(ps.x, ps.y, t.translation.z);
                            t.rotation = Quat::from_rotation_z(ps.rotation);
                            // чистим подтверждённые инпуты
                            while let Some(front) = pending.0.front() {
                                if front.seq <= *ack {
                                    pending.0.pop_front();
                                } else {
                                    break;
                                }
                            }
                            // re‑simulate оставшиеся
                            for inp in pending.0.iter() {
                                simulate_input(&mut *t, inp);
                            }
                        }
                    }
                }

                // 3) Спавним **всех новых** игроков прямо из этого снапшота
                for p in &snap.players {
                    let id = p.id;

                    if dead.0.contains(&id) {
                        // если помечен мёртвым — пропускаем
                        continue;
                    }

                    if spawned.0.insert(p.id) {
                        let label = String::from_str("snapshot").unwrap();
                        spawn_player(&mut commands, &my, id, p.x, p.y, p.rotation, label);
                    }

                    if !hp_ui_map.0.contains_key(&p.id) {
                        let entity = spawn_hp_ui(&mut commands, p.id, p.hp as u32, font.0.clone());
                        hp_ui_map.0.insert(p.id, entity);
                    }
                }

                // 5) Кладем в буфер (для интерполяции)
                buffer.snapshots.push_back(snap);
                while buffer.snapshots.len() > 120 {
                    buffer.snapshots.pop_front();
                }
            }
            // ===================================================
            // 2) СТРЕЛЬБА
            // ===================================================
            // S2C::ShootFx(fx) => {
            //     println!("💥 [Client] got FX from {} at {:?}", fx.shooter_id, fx.from);
            //     if fx.shooter_id != my.id {
            //         commands.spawn((
            //             Sprite {
            //                 color: Color::WHITE,
            //                 custom_size: Some(Vec2::new(12.0, 2.0)),
            //                 ..default()
            //             },
            //             Transform::from_translation(fx.from.extend(10.0))
            //                 .with_rotation(Quat::from_rotation_z(fx.dir.y.atan2(fx.dir.x))),
            //             GlobalTransform::default(),
            //             Bullet {
            //                 ttl: BULLET_TTL,
            //                 vel: fx.dir * BULLET_SPEED,
            //             },
            //         ));
            //     }
            // }
            S2C::ShootFx(fx) => {
                // рисуем только чужие трассеры
                // if fx.shooter_id != my.id {
                // макс. дальность = скорость * ttl
                let max_dist = BULLET_SPEED * BULLET_TTL;
                let dir = fx.dir.normalize_or_zero();

                // расстояние до первой стены; берём из кэша AABB
                let hit_dist = raycast_to_walls_cached(fx.from, dir, max_dist, &wall_cache.0);

                // если стена прямо у дула — не спавним пулю
                if hit_dist <= 0.5 {
                    // info!("🔫 tracer blocked immediately");
                } else {
                    // обрезаем трассер по стене: ttl = dist / speed
                    let ttl = hit_dist / BULLET_SPEED;

                    commands.spawn((
                        Sprite {
                            color: Color::WHITE,
                            custom_size: Some(Vec2::new(12.0, 2.0)),
                            ..default()
                        },
                        Transform::from_translation(fx.from.extend(10.0))
                            .with_rotation(Quat::from_rotation_z(dir.y.atan2(dir.x))),
                        GlobalTransform::default(),
                        Bullet {
                            ttl,
                            vel: dir * BULLET_SPEED,
                        },
                    ));
                }
                // }
            }
            // ===================================================
            // 2) СПАВН ИГРОКА (новый или респавн)
            // ===================================================
            S2C::PlayerConnected { id, x, y } | S2C::PlayerRespawn { id, x, y } => {
                dead.0.remove(&id);

                // сброс буфера снапшотов → сразу телепорт, без интерполяции
                buffer.snapshots.clear();

                // 1) Если этот id уже есть — деспавним старую сущность
                if spawned.0.remove(&id) {
                    for (ent, marker) in q_marker.iter() {
                        if marker.0 == id {
                            commands.entity(ent).despawn();
                            break;
                        }
                    }
                }

                // spawn_hp_ui(&mut commands, id, 100, font.0.clone());

                let label = String::from_str("new/respawn").unwrap();
                spawn_player(&mut commands, &my, id, x, y, 0.0, label);

                spawned.0.insert(id);
            }
            // ===================================================
            // 2) ИГРОК ВЫШЕЛ
            // ===================================================
            S2C::PlayerLeft(left_id) => {
                dead.0.remove(&left_id);

                if let Some((entity, _)) = q_marker.iter().find(|(_, marker)| marker.0 == left_id) {
                    // 1) сразу удаляем сущность
                    commands.entity(entity).despawn();
                    spawned.0.remove(&left_id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", left_id);
                }

                events.p2().write(PlayerLeftEvent(left_id));
            }
            // ===================================================
            // 2) ИГРОК ВЫШЕЛ 2 (event disconnect)
            // ===================================================
            S2C::PlayerDisconnected { id } => {
                dead.0.remove(&id);

                if let Some((entity, _)) = q_marker.iter().find(|(_, marker)| marker.0 == id) {
                    // 1) сразу удаляем сущность
                    commands.entity(entity).despawn();
                    spawned.0.remove(&id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", id);
                }

                events.p2().write(PlayerLeftEvent(id));
            }
            // ===================================================
            // 2) PONG
            // ===================================================
            S2C::Pong {
                client_time,
                server_time,
            } => {
                let now = time_in_seconds();
                let rtt = now - client_time;
                let one_way = (rtt - (now - server_time)) * 0.5;
                latency.rtt = rtt;
                latency.offset = server_time - (client_time + one_way);
                // todo revert
                // info!("💓 Pong: RTT={:.3}s, offset={:.3}s", rtt, latency.offset);
            }
            // ===================================================
            // 2) УРОН НАНЕСЕН
            // ===================================================
            S2C::PlayerDamaged { id, new_hp, damage } => {
                // println!("Игрок {id} получил {damage} урона, осталось {new_hp} HP");
                events.p0().write(PlayerDamagedEvent { id, new_hp, damage });
            }
            // ===================================================
            // Спавн гранаты
            // ===================================================
            S2C::GrenadeSpawn(ev) => {
                let printable_ev = ev.clone();
                events.p3().write(GrenadeSpawnEvent(ev));

                info!("💣 GrenadeSpawn {}", printable_ev.id);
            }
            // ===================================================
            // Снапшот гранаты (позиция/скорость)
            // ===================================================
            S2C::GrenadeSync { id, pos, vel, ts } => {
                info!("SYNC GRENADES: {:?}", pos);
                let e = grenade_states.0.entry(id).or_default();
                *e = NetState {
                    pos,
                    vel,
                    ts,
                    has: true,
                };
            }
            // ===================================================
            // 2) Взрыв гранаты
            // ===================================================
            S2C::GrenadeDetonated { id, pos } => {
                events.p4().write(GrenadeDetonatedEvent { id, pos });
            }
            // ===================================================
            // 2) СМЕРТЬ
            // ===================================================
            S2C::PlayerDied { victim, killer } => {
                info!("[Client]   PlayerDied victim={}", victim);

                // помечаем убитого «мертвым»
                dead.0.insert(victim);

                // если это мы — despawn своего спрайта
                if victim == my.id {
                    for (ent, _) in q_marker.iter().filter(|(_, m)| m.0 == victim) {
                        commands.entity(ent).despawn();
                        spawned.0.remove(&victim);
                    }
                    // 2) сбрасываем все старые снапшоты, чтобы не воскрешать
                    buffer.snapshots.clear();
                    // можно показать UI‑фразу или эффект «вы умерли»
                }
                // если это кто‑то другой — despawn его квадрат
                else if let Some((ent, _)) = q_marker.iter().find(|(_, m)| m.0 == victim) {
                    commands.entity(ent).despawn();
                    spawned.0.remove(&victim);
                }

                events.p1().write(PlayerDied {
                    victim: victim,
                    killer: killer,
                });

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
) {
    let tf = Transform::from_xyz(x, y, 0.0).with_rotation(Quat::from_rotation_z(rot));
    if id == me.id {
        // локальный (зелёный)
        commands.spawn((
            Sprite {
                color: Color::srgb(0.0, 1.0, 0.0),
                custom_size: Some(Vec2::splat(PLAYER_SIZE)),
                ..default()
            },
            tf,
            GlobalTransform::default(),
            PlayerMarker(id),
            LocalPlayer,
        ));

        info!("[Client]{from} spawn LOCAL {}", id);
    } else {
        // чужой (синий)
        commands.spawn((
            Sprite {
                color: Color::srgb(0.2, 0.4, 1.0),
                custom_size: Some(Vec2::splat(PLAYER_SIZE)),
                ..default()
            },
            tf,
            GlobalTransform::default(),
            PlayerMarker(id),
        ));
        info!("[Client][{from}] spawn REMOTE {}", id);
    }
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
        info!("apply id={} pos={:?}", net.id, tf.translation.truncate());

        if let Some(s) = states.0.get(&net.id) {
            if !s.has {
                continue;
            }
            let mut dt = (now_server - s.ts) as f32;
            if !time_sync.offset.is_finite() {
                dt = 0.0;
            } // до первого Snapshot
            dt = dt.clamp(0.0, 0.25); // анти-скачок
            let pos = s.pos + s.vel * dt;
            tf.translation.x = pos.x;
            tf.translation.y = pos.y;
        }
    }
}
