use std::str::FromStr;

use crate::components::{Bullet, GrenadeMarker, GrenadeTimer, LocalPlayer, PlayerMarker};
use crate::resources::{
    ClientLatency, DeadPlayers, MyPlayer, PendingInputsClient, SnapshotBuffer, SpawnedPlayers,
    TimeSync,
};
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_S2C, MOVE_SPEED, TICK_DT};
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
                        // это новый для нас игрок
                        // let tf = Transform::from_xyz(p.x, p.y, 0.0)
                        //     .with_rotation(Quat::from_rotation_z(p.rotation));

                        let label = String::from_str("snapshot").unwrap();
                        spawn_player(&mut commands, &my, id, p.x, p.y, p.rotation, label);

                        // if p.id == my.id {
                        //     info!("[Network] spawn LOCAL {}", p.id);
                        //     commands.spawn((
                        //         Sprite {
                        //             color: Color::srgb(0.0, 1.0, 0.0),
                        //             custom_size: Some(Vec2::splat(40.0)),
                        //             ..default()
                        //         },
                        //         tf,
                        //         GlobalTransform::default(),
                        //         PlayerMarker(p.id),
                        //         LocalPlayer,
                        //     ));
                        // } else {
                        //     info!("[Network] spawn REMOTE {}", p.id);
                        //     commands.spawn((
                        //         Sprite {
                        //             color: Color::srgb(0.2, 0.4, 1.0),
                        //             custom_size: Some(Vec2::splat(40.0)),
                        //             ..default()
                        //         },
                        //         tf,
                        //         GlobalTransform::default(),
                        //         PlayerMarker(p.id),
                        //     ));
                        // }
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
            S2C::ShootFx(fx) => {
                println!("💥 [Client] got FX from {} at {:?}", fx.shooter_id, fx.from);
                if fx.shooter_id != my.id {
                    commands.spawn((
                        Sprite {
                            color: Color::WHITE,
                            custom_size: Some(Vec2::new(12.0, 2.0)),
                            ..default()
                        },
                        Transform::from_translation(fx.from.extend(10.0))
                            .with_rotation(Quat::from_rotation_z(fx.dir.y.atan2(fx.dir.x))),
                        GlobalTransform::default(),
                        Bullet {
                            ttl: 0.35,
                            vel: fx.dir * 900.0,
                        },
                    ));
                }
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

                // 2) Если это мы — сбрасываем буфер снапшотов
                // if id == my.id {
                // buffer.snapshots.clear();
                    // buffer.delay = DEFAULT_INTERP_DELAY; // или как у вас настроено
                // }

                // 3) Спавним нового
                // let tf = Transform::from_xyz(x, y, 0.0);

                let label = String::from_str("new/respawn").unwrap();
                spawn_player(&mut commands, &my, id, x, y, 0.0, label);

                spawned.0.insert(id);

                // if id == my.id {
                //     // спавним локального
                //     commands.spawn((
                //         Sprite {
                //             color: Color::srgb(0.0, 1.0, 0.0),
                //             custom_size: Some(Vec2::splat(40.0)),
                //             ..default()
                //         },
                //         tf,
                //         GlobalTransform::default(),
                //         PlayerMarker(id),
                //         LocalPlayer,
                //     ));
                //     info!("🔄 Я ({}) респавнился", id);
                // } else if spawned.0.insert(id) {
                //     // спавним чужого
                //     commands.spawn((
                //         Sprite {
                //             color: Color::srgb(0.2, 0.4, 1.0),
                //             custom_size: Some(Vec2::splat(40.0)),
                //             ..default()
                //         },
                //         tf,
                //         GlobalTransform::default(),
                //         PlayerMarker(id),
                //     ));
                //     info!("🔄 Игрок {} респавнился", id);
                // }
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
            // 2) Спавн гранаты
            // ===================================================
            S2C::GrenadeSpawn(ev) => {
                commands.spawn((
                    GrenadeMarker(ev.id),
                    Sprite {
                        color: Color::WHITE,
                        custom_size: Some(Vec2::new(12.0, 2.0)),
                        ..default()
                    },
                    Transform::from_translation(ev.from.extend(10.0))
                        .with_rotation(Quat::from_rotation_z(ev.dir.y.atan2(ev.dir.x))),
                    GlobalTransform::default(),
                    GrenadeTimer(Timer::from_seconds(ev.timer, TimerMode::Once)),
                ));
                info!("💣 GrenadeSpawn {}", ev.id);
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

/// Утилита для единообразного создания сущности игрока.
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
                custom_size: Some(Vec2::splat(40.0)),
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
                custom_size: Some(Vec2::splat(40.0)),
                ..default()
            },
            tf,
            GlobalTransform::default(),
            PlayerMarker(id),
        ));
        info!("[Client][{from}] spawn REMOTE {}", id);
    }
}
