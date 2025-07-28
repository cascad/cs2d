use crate::components::{Bullet, GrenadeMarker, GrenadeTimer, LocalPlayer, PlayerMarker};
use crate::resources::{
    ClientLatency, InitialSpawnDone, MyPlayer, PendingInputsClient, SnapshotBuffer, SpawnedPlayers,
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
    my: Res<MyPlayer>,
    mut pending: ResMut<PendingInputsClient>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
    mut spawned: ResMut<SpawnedPlayers>,
    // todo fix q and query
    query: Query<(Entity, &PlayerMarker)>,
    mut latency: ResMut<ClientLatency>,
    mut init_done: ResMut<InitialSpawnDone>,
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
                // Time sync
                if buffer.snapshots.is_empty() {
                    time_sync.offset = time_in_seconds() - snap.server_time;
                }
                // Reconciliation
                if let Ok(mut t) = q.single_mut() {
                    if let Some(ack) = snap.last_input_seq.get(&my.id) {
                        if let Some(ps) = snap.players.iter().find(|p| p.id == my.id) {
                            t.translation = Vec3::new(ps.x, ps.y, t.translation.z);
                            t.rotation = Quat::from_rotation_z(ps.rotation);
                            while let Some(front) = pending.0.front() {
                                if front.seq <= *ack {
                                    pending.0.pop_front();
                                } else {
                                    break;
                                }
                            }
                            for inp in pending.0.iter() {
                                simulate_input(&mut *t, inp);
                            }
                        }
                    }
                }

                // info!("[Network] checking initial spawn…");
                // единовременный бэкофил: спавним всех, включая локального
                if !init_done.0 && my.got {
                    info!(
                        "[Network] → Performing initial back‑fill of {} players",
                        snap.players.len()
                    );

                    let mut spawned_any = false; // ← новый флажок

                    for p in &snap.players {
                        // отметим, что этого игрока мы уже спавнили
                        if spawned.0.insert(p.id) {
                            spawned_any = true; // ← отметили, что хоть один игрок появился

                            let tf = Transform::from_xyz(p.x, p.y, 0.0)
                                .with_rotation(Quat::from_rotation_z(p.rotation));
                            // Логируем факт спавна
                            info!(
                                "[Network]   Spawning player {} at ({:.1},{:.1})",
                                p.id, p.x, p.y
                            );

                            // собираем нужный bundle
                            if p.id == my.id {
                                // локальный
                                commands.spawn((
                                    Sprite {
                                        color: Color::srgb(0.0, 1.0, 0.0),
                                        custom_size: Some(Vec2::splat(40.0)),
                                        ..default()
                                    },
                                    tf,
                                    GlobalTransform::default(),
                                    PlayerMarker(p.id),
                                    LocalPlayer,
                                ));
                            } else {
                                // чужой
                                commands.spawn((
                                    Sprite {
                                        color: Color::srgb(0.2, 0.4, 1.0),
                                        custom_size: Some(Vec2::splat(40.0)),
                                        ..default()
                                    },
                                    tf,
                                    GlobalTransform::default(),
                                    PlayerMarker(p.id),
                                ));
                            };

                            info!("[InitBackfill] Spawned {} at ({:.1},{:.1})", p.id, p.x, p.y);
                        } else {
                            info!("[Network]   Already spawned player {}", p.id);
                        }
                    }
                    // Флаг переносим ВНЕ цикла:
                    if spawned_any {
                        info!("[Network] initial back‑fill finished");
                        init_done.0 = true;
                    } else {
                        info!("[Network] snapshot пустой, ждём следующий");
                    }
                }

                // Buffer for interpolation
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
            // 2) ИГРОК ВЫШЕЛ
            // ===================================================
            S2C::PlayerLeft(left_id) => {
                if let Some((entity, _)) = query.iter().find(|(_, marker)| marker.0 == left_id) {
                    // 1) сразу удаляем сущность
                    commands.entity(entity).despawn();
                    spawned.0.remove(&left_id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", left_id);

                    // 2) сбрасываем все старые снапшоты, чтобы не воскрешать
                    buffer.snapshots.clear();
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
                // если это мы — despawn своего спрайта
                if victim == my.id {
                    for (ent, _) in query.iter().filter(|(_, m)| m.0 == victim) {
                        commands.entity(ent).despawn();
                        spawned.0.remove(&victim);
                    }
                    // можно показать UI‑фразу или эффект «вы умерли»
                }
                // если это кто‑то другой — despawn его квадрат
                else if let Some((ent, _)) = query.iter().find(|(_, m)| m.0 == victim) {
                    commands.entity(ent).despawn();
                    spawned.0.remove(&victim);
                }
                // 2) сбрасываем все старые снапшоты, чтобы не воскрешать
                buffer.snapshots.clear();

                info!("💀 Игрок {} погиб ({:?})", victim, killer);
            }
            // ===================================================
            // 3) РЕСПАУН
            // ===================================================
            S2C::PlayerRespawn { id, x, y } => {
                info!("[Client]   PlayerRespawn id={} at ({},{})", id, x, y);
                // респавнимся именно в переданной точке
                let tf = Transform::from_xyz(x, y, 0.0);
                if id == my.id {
                    // спавним локального
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
                    info!("🔄 Я ({}) респавнился", id);
                } else if spawned.0.insert(id) {
                    // спавним чужого
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
                    info!("🔄 Игрок {} респавнился", id);
                }
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
