use crate::resources::{
    AppliedSeqs, GrenadeState, Grenades, LastHeard, PendingInputs, PlayerStates, SnapshotHistory,
};
use crate::utils::{check_hit_lag_comp, push_history};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::constants::{CH_C2S, CH_S2C};
use protocol::messages::{C2S, S2C, ShootFx};

pub fn process_c2s_messages(
    mut server: ResMut<QuinnetServer>,
    mut pending: ResMut<PendingInputs>,
    mut states: ResMut<PlayerStates>,
    mut last_heard: ResMut<LastHeard>,
    mut applied: ResMut<AppliedSeqs>,
    mut history: ResMut<SnapshotHistory>,
    mut grenades: ResMut<Grenades>,
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
                C2S::Shoot(shoot) => {
                    println!("🔫 [Server] ShootEvent from {}: {:?}", client_id, shoot);
                    if let Some(hit) = check_hit_lag_comp(&history.buf, &states.0, &shoot) {
                        println!("💥 [Server] hit target {}", hit);
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
                    // сразу кидаем в ресурс Grenades
                    println!("💣 [Server] ThrowGrenade from {} at {:?} (t={})", 
                    client_id, ev.from, ev.timestamp);
                    grenades.0.insert(
                        ev.id,
                        GrenadeState {
                            ev: ev.clone(),
                            created: now,
                        },
                    );

                    let grenade_id = ev.id;
                    // и рассылаем всем клиентам, чтобы они визуализировали гранату
                    endpoint
                        .broadcast_message_on(CH_S2C, S2C::GrenadeSpawn(ev))
                        .ok();
                    info!("💣 Клиент {} бросил гранату {}", client_id, grenade_id);
                }
            }
        }
    }

    push_history(&mut history, now, &states.0);
}
