use crate::events::DamageEvent;
use crate::resources::{
    AppliedSeqs, GrenadeState, Grenades, LastGrenadeThrows, LastHeard, PendingInputs, PlayerStates,
    SnapshotHistory, WallGridRes,
};
use crate::utils::check_hit_lag_comp;
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::abilities::AbilityConfig;
use protocol::combat::in_melee_arc;
use protocol::constants::{
    CH_C2S, CH_S2C, GRENADE_RADIUS, GRENADE_USAGE_COOLDOWN, HITBOX_RADIUS, MAX_LAG_COMP,
    MELEE_DAMAGE, MELEE_HALF_ANGLE, MELEE_RANGE, SHOOT_RIFLE_DAMAGE,
};
use protocol::messages::{C2S, GrenadeEvent, S2C, ShootFx};

pub fn process_c2s_messages(
    mut server: ResMut<QuinnetServer>,
    mut pending: ResMut<PendingInputs>,
    mut states: ResMut<PlayerStates>,
    mut last_heard: ResMut<LastHeard>,
    mut applied: ResMut<AppliedSeqs>,
    history: Res<SnapshotHistory>,
    mut grenades: ResMut<Grenades>,
    mut last_grenade: ResMut<LastGrenadeThrows>,
    mut damage_events: EventWriter<DamageEvent>,
    walls: Res<WallGridRes>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let endpoint = server.endpoint_mut();

    for client_id in endpoint.clients() {
        while let Some((chan, msg)) = endpoint.try_receive_message_from::<C2S>(client_id) {
            debug_assert_eq!(chan, CH_C2S);

            // помечаем время последнего сообщения
            last_heard.0.insert(client_id, now);

            match msg {
                C2S::Input(input) => {
                    pending.0.entry(client_id).or_default().push_back(input);
                }
                C2S::Shoot(mut shoot) => {
                    // АНТИ-ЧИТ: не доверяем shooter_id из пакета — берём id соединения.
                    // Иначе можно было бы «стрелять от лица» другого игрока.
                    shoot.shooter_id = client_id;
                    // АНТИ-ЧИТ: ограничиваем откат лаг-компенсации, чтобы нельзя было
                    // «отмотать» произвольно далеко (бэктрек по старому timestamp).
                    shoot.timestamp = shoot.timestamp.clamp(now - MAX_LAG_COMP, now);

                    // println!("🔫 [Server] ShootEvent from {}: {:?}", client_id, shoot);
                    if let Some(hit) = check_hit_lag_comp(&history.buf, &states.0, &shoot, &walls.0)
                    {
                        println!("💥 [Server] hit target {}", hit);

                        damage_events.write(DamageEvent {
                            target: hit,
                            amount: SHOOT_RIFLE_DAMAGE as i32,
                            source: Some(shoot.shooter_id),
                        });
                    }

                    if let Some(st) = states.0.get(&shoot.shooter_id) {
                        let fx = ShootFx {
                            shooter_id: shoot.shooter_id,
                            from: st.pos, // используем позицию игрока из состояния
                            dir: shoot.dir,
                            timestamp: shoot.timestamp,
                        };
                        endpoint
                            .broadcast_message_on(CH_S2C, S2C::ShootFx(fx))
                            .unwrap();
                    }
                }
                C2S::Heartbeat => {
                    // ничего более не делаем, выше уже есть HB
                }
                // Клиент корректно сообщил, что уходит
                C2S::Goodbye => {
                    states.0.remove(&client_id);
                    pending.0.remove(&client_id);
                    applied.0.remove(&client_id);

                    endpoint
                        .broadcast_message_on(CH_S2C, S2C::PlayerLeft(client_id))
                        .ok();

                    info!("👋 Клиент {client_id} ушёл - broadcast PlayerLeft");
                }
                C2S::Ping(client_ts) => {
                    let server_ts = time.elapsed_secs_f64();
                    // сразу отвечаем клиенту,
                    // подставляем обе метки, чтобы он посчитал RTT и смещение
                    endpoint
                        .send_message_on(
                            client_id,
                            CH_S2C,
                            S2C::Pong {
                                client_time: client_ts,
                                server_time: server_ts,
                            },
                        )
                        .ok();
                }
                C2S::ThrowGrenade(ev) => {
                    let cooldown = GRENADE_USAGE_COOLDOWN;

                    let can_throw = match last_grenade.map.get(&client_id) {
                        Some(&last_time) => now - last_time >= cooldown,
                        None => true,
                    };

                    if !can_throw {
                        info!(
                            "⏳ Client {} tried to throw grenade before cooldown finished",
                            client_id
                        );
                        continue; // Пропускаем бросок
                    }

                    // Обновляем время последнего броска
                    last_grenade.map.insert(client_id, now);

                    // Нормализуем присланный вектор (на всякий случай)
                    let mut dir = ev.dir;
                    if dir.length_squared() <= f32::EPSILON {
                        // мусорный ввод — игнорим
                        continue;
                    }
                    dir = dir.normalize();

                    // Смещаем точку спавна вперёд по направлению (радиус + небольшой запас),
                    // чтобы не родиться впритык к стене/игроку
                    let spawn_from = ev.from + dir * (GRENADE_RADIUS + 1.0);

                    // Заводим серверное состояние
                    grenades.0.insert(
                        ev.id,
                        GrenadeState {
                            ev: GrenadeEvent {
                                id: ev.id,
                                from: spawn_from,
                                dir,             // нормализованный
                                speed: ev.speed, // фактический из клиента (или оставь константу, если у тебя фикс)
                                timer: ev.timer, // фактический из клиента
                                timestamp: ev.timestamp,
                            },
                            created: now,
                            pos: spawn_from,
                            vel: dir * ev.speed,
                        },
                    );

                    let grenade_id = ev.id;
                    // и рассылаем всем клиентам, чтобы они визуализировали гранату
                    let _ = endpoint.broadcast_message_on(
                        CH_S2C,
                        S2C::GrenadeSpawn(GrenadeEvent {
                            id: ev.id,
                            from: spawn_from,
                            dir,
                            speed: ev.speed, // не подменяем на константу
                            timer: ev.timer,
                            timestamp: ev.timestamp,
                        }),
                    );

                    info!("💣 Клиент {} бросил гранату {}", client_id, grenade_id);
                }
                C2S::Melee(ev) => {
                    let cfg = AbilityConfig::default();

                    // атакующий должен существовать и быть готов (кулдаун + стамина)
                    let (from, rot, ready) = match states.0.get(&client_id) {
                        Some(a) => (a.pos, a.rot, a.abilities.melee_ready(&cfg)),
                        None => continue,
                    };
                    if !ready {
                        continue;
                    }

                    let dir = if ev.dir.length_squared() > f32::EPSILON {
                        ev.dir.normalize()
                    } else {
                        Vec2::new(rot.cos(), rot.sin())
                    };

                    // цели в секторе перед атакующим, не закрытые стеной
                    let mut victims: Vec<u64> = Vec::new();
                    for (&tid, tst) in states.0.iter() {
                        if tid == client_id {
                            continue;
                        }
                        if in_melee_arc(from, dir, tst.pos, HITBOX_RADIUS, MELEE_RANGE, MELEE_HALF_ANGLE)
                            && !walls.0.segment_blocked(from, tst.pos, 0.001)
                        {
                            victims.push(tid);
                        }
                    }

                    // фиксируем удар (кулдаун + стамина) и наносим урон
                    if let Some(att) = states.0.get_mut(&client_id) {
                        att.abilities.consume_melee(&cfg);
                    }
                    for v in &victims {
                        damage_events.write(DamageEvent {
                            target: *v,
                            amount: MELEE_DAMAGE as i32,
                            source: Some(client_id),
                        });
                    }

                    endpoint
                        .broadcast_message_on(
                            CH_S2C,
                            S2C::MeleeFx {
                                attacker_id: client_id,
                                from,
                                dir,
                            },
                        )
                        .ok();
                }
            }
        }
    }
    // История состояний пишется в server_tick (раз в тик). Здесь повторная запись
    // только плодила бы клон всей карты состояний каждый кадр.
}
