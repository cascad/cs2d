use crate::events::{DamageEvent, NpcDamageEvent};
use crate::resources::{
    AppliedSeqs, GrenadeState, Grenades, LastGrenadeThrows, LastHeard, Npcs, PendingInputs,
    PendingMelee, PendingMelees, PlayerStates, SnapshotHistory, WallGridRes,
};
use crate::utils::check_hit_lag_comp;
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::combat::in_melee_sector;
use protocol::constants::{
    CH_C2S, CH_S2C, GRENADE_RADIUS, GRENADE_USAGE_COOLDOWN, HITBOX_RADIUS, MAX_LAG_COMP,
    MELEE_DAMAGE, MELEE_HALF_ANGLE, MELEE_HALF_WIDTH, MELEE_RANGE, NPC_RADIUS,
    SHOOT_RIFLE_DAMAGE,
};
use protocol::messages::{C2S, GrenadeEvent, S2C, ShootFx};

/// Анти-DoS: максимум входящих сообщений, обрабатываемых от одного клиента за один
/// вызов системы. Флудер не сможет загнать сервер в busy-loop — лишнее отбрасываем,
/// оно либо подождёт следующего тика, либо отвалится по лимитам очереди транспорта.
const MAX_MSGS_PER_CLIENT_PER_TICK: usize = 256;
/// Анти-DoS: максимум буферизованных инпутов на клиента. `server_tick` всё равно
/// берёт только последний (`queue.back()`), поэтому держать больше бессмысленно —
/// при переполнении выкидываем самые старые.
const MAX_PENDING_INPUTS: usize = 16;

pub fn process_c2s_messages(
    mut server: ResMut<QuinnetServer>,
    mut pending: ResMut<PendingInputs>,
    mut states: ResMut<PlayerStates>,
    mut last_heard: ResMut<LastHeard>,
    mut applied: ResMut<AppliedSeqs>,
    history: Res<SnapshotHistory>,
    mut grenades: ResMut<Grenades>,
    mut last_grenade: ResMut<LastGrenadeThrows>,
    mut damage_events: MessageWriter<DamageEvent>,
    walls: Res<WallGridRes>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let endpoint = server.endpoint_mut();

    for client_id in endpoint.clients() {
        let mut processed = 0usize;
        while let Some(msg) = endpoint.try_receive_message_from::<C2S, _>(client_id, CH_C2S) {
            // АНТИ-DoS: ограничиваем число сообщений от одного клиента за тик.
            processed += 1;
            if processed > MAX_MSGS_PER_CLIENT_PER_TICK {
                warn!("client {client_id}: превышен лимит сообщений за тик — троттлим");
                break;
            }

            // помечаем время последнего сообщения
            last_heard.0.insert(client_id, now);

            match msg {
                C2S::Input(input) => {
                    let q = pending.0.entry(client_id).or_default();
                    q.push_back(input);
                    // держим только свежие инпуты (server_tick всё равно берёт back()).
                    while q.len() > MAX_PENDING_INPUTS {
                        q.pop_front();
                    }
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
                            source_pos: None,
                            npc_source: None,
                        });
                    }

                    if let Some(st) = states.0.get(&shoot.shooter_id) {
                        let fx = ShootFx {
                            shooter_id: shoot.shooter_id,
                            from: st.pos, // используем позицию игрока из состояния
                            dir: shoot.dir,
                            timestamp: shoot.timestamp,
                        };
                        if let Err(e) = endpoint.broadcast_message_on(CH_S2C, S2C::ShootFx(fx)) {
                            warn!("broadcast ShootFx failed: {e:?}");
                        }
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
            }
        }
    }
    // История состояний пишется в server_tick (раз в тик). Здесь повторная запись
    // только плодила бы клон всей карты состояний каждый кадр.
}

/// Наносит урон от запланированных ударов, когда подошёл момент «середины
/// взмаха». Зона — 90°-сектор с постоянной шириной (не сужается вблизи); цели
/// считаем по их ТЕКУЩИМ позициям.
pub fn resolve_melees(
    mut pending: ResMut<PendingMelees>,
    states: Res<PlayerStates>,
    npcs: Res<Npcs>,
    walls: Res<WallGridRes>,
    time: Res<Time>,
    mut damage_events: MessageWriter<DamageEvent>,
    mut npc_damage_events: MessageWriter<NpcDamageEvent>,
) {
    let now = time.elapsed_secs_f64();
    // запас по радиусу неписи: они движутся и видны клиенту с задержкой интерполяции
    const NPC_MELEE_HITBOX: f32 = HITBOX_RADIUS + NPC_RADIUS;

    let mut keep: Vec<PendingMelee> = Vec::with_capacity(pending.0.len());
    for m in pending.0.drain(..) {
        if now < m.resolve_at {
            keep.push(m);
            continue;
        }
        // атакующий мог отключиться/умереть — тогда удар «растворяется»
        let Some(att) = states.0.get(&m.attacker) else { continue };
        let from = att.pos; // строго от ЦЕНТРА модели

        for (&tid, tst) in states.0.iter() {
            if tid == m.attacker {
                continue;
            }
            if in_melee_sector(
                from,
                m.dir,
                tst.pos,
                HITBOX_RADIUS,
                MELEE_RANGE,
                MELEE_HALF_ANGLE,
                MELEE_HALF_WIDTH,
            ) && !walls.0.segment_blocked(from, tst.pos, 0.001)
            {
                damage_events.write(DamageEvent {
                    target: tid,
                    amount: MELEE_DAMAGE as i32,
                    source: Some(m.attacker),
                    source_pos: None,
                    npc_source: None,
                });
            }
        }
        for (&nid, npc) in npcs.0.iter() {
            if in_melee_sector(
                from,
                m.dir,
                npc.pos,
                NPC_MELEE_HITBOX,
                MELEE_RANGE,
                MELEE_HALF_ANGLE,
                MELEE_HALF_WIDTH,
            ) && !walls.0.segment_blocked(from, npc.pos, 0.001)
            {
                npc_damage_events.write(NpcDamageEvent {
                    target: nid,
                    amount: MELEE_DAMAGE as i32,
                });
            }
        }
    }
    pending.0 = keep;
}
