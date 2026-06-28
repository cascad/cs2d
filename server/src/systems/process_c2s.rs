use crate::events::{DamageEvent, NpcDamageEvent};
use crate::resources::{
    Accounts, AppliedSeqs, GrenadeState, Grenades, LastGrenadeThrows, LastHeard, Npcs,
    PendingInputs, PendingMelee, PendingMelees, PlayerState, PlayerStates, SnapshotHistory,
    SpawnPoints, SpawnedClients, WallGridRes,
};
use crate::scoreboard::AuthOutcome;
use crate::systems::spawn::pick_spawn_point;
use crate::utils::check_hit_lag_comp;
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::combat::in_melee_sector;
use protocol::constants::{
    grenade_max_reach, CH_C2S, CH_S2C, GRENADE_RADIUS, GRENADE_SPEED, GRENADE_TIMER,
    GRENADE_USAGE_COOLDOWN, HITBOX_RADIUS, MAX_LAG_COMP, MELEE_DAMAGE, MELEE_HALF_ANGLE,
    MELEE_HALF_WIDTH, MELEE_RANGE, NPC_RADIUS, SHOOT_RIFLE_DAMAGE,
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
    mut accounts: ResMut<Accounts>,
    mut spawned: ResMut<SpawnedClients>,
    spawns: Res<SpawnPoints>,
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

            // АВТОРИЗАЦИЯ-ГЕЙТ: до успешного Hello принимаем ТОЛЬКО Hello/Ping/
            // Heartbeat/Goodbye. Игровые сообщения (Input/Shoot/Throw) от
            // неавторизованного соединения игнорируем — играть может лишь тот,
            // кого сервер заспавнил после проверки аккаунта.
            let authed = accounts.0.is_online(client_id);
            match msg {
                C2S::Hello { name, password } => {
                    if authed {
                        continue; // повторный Hello уже авторизованного — игнор
                    }
                    let outcome = accounts.0.authenticate(client_id, &name, &password);
                    match outcome {
                        AuthOutcome::Registered | AuthOutcome::Authenticated => {
                            let pos = pick_spawn_point(&spawns, client_id);
                            states.0.insert(
                                client_id,
                                PlayerState {
                                    pos,
                                    rot: 0.0,
                                    stance: Default::default(),
                                    hp: 100,
                                    ..Default::default()
                                },
                            );
                            spawned.0.insert(client_id);

                            let disp_name = accounts
                                .0
                                .display_name(client_id)
                                .unwrap_or("?")
                                .to_string();
                            info!("✅ {client_id} вошёл как '{disp_name}' ({outcome:?})");

                            endpoint
                                .send_message_on(
                                    client_id,
                                    CH_S2C,
                                    S2C::AuthOk { your_id: client_id },
                                )
                                .ok();
                            endpoint
                                .broadcast_message_on(
                                    CH_S2C,
                                    S2C::PlayerConnected {
                                        id: client_id,
                                        x: pos.x,
                                        y: pos.y,
                                    },
                                )
                                .ok();
                            endpoint
                                .broadcast_message_on(
                                    CH_S2C,
                                    S2C::Scoreboard(accounts.0.snapshot()),
                                )
                                .ok();
                        }
                        AuthOutcome::WrongPassword | AuthOutcome::AlreadyOnline => {
                            let reason = match outcome {
                                AuthOutcome::WrongPassword => "Неверный пароль",
                                _ => "Аккаунт уже в игре",
                            };
                            info!("⛔ {client_id} отклонён: {reason}");
                            endpoint
                                .send_message_on(
                                    client_id,
                                    CH_S2C,
                                    S2C::AuthDenied {
                                        reason: reason.to_string(),
                                    },
                                )
                                .ok();
                            // Соединение НЕ рвём принудительно здесь: иначе только
                            // что поставленный в очередь AuthDenied мог бы не успеть
                            // уйти. Клиент сам закроется по AuthDenied, а до тех пор
                            // он всё равно ничего не может (игровые сообщения от
                            // неавторизованного соединения игнорируются гейтом).
                        }
                    }
                }
                C2S::Input(_) | C2S::Shoot(_) | C2S::ThrowGrenade(_) if !authed => {
                    // неавторизованное соединение: игнорируем игровые сообщения
                }
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

                    // Выход = смерть без убийцы; статистику аккаунта сохраняем.
                    let changed = if accounts.0.is_online(client_id) {
                        accounts.0.add_death(client_id);
                        accounts.0.unbind(client_id);
                        true
                    } else {
                        false
                    };

                    endpoint
                        .broadcast_message_on(CH_S2C, S2C::PlayerLeft(client_id))
                        .ok();
                    if changed {
                        endpoint
                            .broadcast_message_on(CH_S2C, S2C::Scoreboard(accounts.0.snapshot()))
                            .ok();
                    }

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

                    // АВТОРИТЕТНОСТЬ СЕРВЕРА: точку броска берём из СЕРВЕРНОЙ позиции
                    // игрока (не доверяем `ev.from`), скорость/таймер — из констант,
                    // а направление считаем из присланной ЦЕЛИ (куда указал курсор).
                    // Клиент задаёт только желаемую точку падения. Так механика
                    // встроена в общий контракт сети, а не «на доверии».
                    let origin = match states.0.get(&client_id) {
                        Some(st) => st.pos,
                        None => continue, // нет состояния игрока — бросать неоткуда
                    };

                    let to_target = ev.target - origin;
                    if to_target.length_squared() <= f32::EPSILON {
                        // цель совпала с игроком — мусорный ввод, игнорим
                        continue;
                    }
                    let dir = to_target.normalize();

                    // Кламп дальности: дальше максимума не улетит; ближе — взорвётся
                    // в указанной точке (через лимит пройденного пути).
                    let reach = grenade_max_reach();
                    let want_dist = to_target.length().min(reach);
                    let clamped_target = origin + dir * want_dist;

                    // Обновляем время последнего броска (после валидации цели)
                    last_grenade.map.insert(client_id, now);

                    // Смещаем точку спавна вперёд по направлению (радиус + небольшой запас),
                    // чтобы не родиться впритык к стене/игроку
                    let spawn_offset = GRENADE_RADIUS + 1.0;
                    let spawn_from = origin + dir * spawn_offset;
                    // Лимит пути считаем ОТ точки спавна (она уже сдвинута вперёд).
                    let travel_limit = (want_dist - spawn_offset).max(0.0);

                    let spawn_ev = GrenadeEvent {
                        id: ev.id,
                        from: spawn_from,
                        dir, // нормализованный
                        target: clamped_target,
                        speed: GRENADE_SPEED,
                        timer: GRENADE_TIMER,
                        timestamp: ev.timestamp,
                    };

                    // Заводим серверное состояние
                    grenades.0.insert(
                        ev.id,
                        GrenadeState {
                            ev: spawn_ev.clone(),
                            owner: client_id,
                            created: now,
                            pos: spawn_from,
                            vel: dir * GRENADE_SPEED,
                            bounces: 0,
                            travel_limit,
                            traveled: 0.0,
                        },
                    );

                    let grenade_id = ev.id;
                    // и рассылаем всем клиентам, чтобы они визуализировали гранату
                    let _ = endpoint.broadcast_message_on(CH_S2C, S2C::GrenadeSpawn(spawn_ev));

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
                    source: Some(m.attacker),
                });
            }
        }
    }
    pending.0 = keep;
}
