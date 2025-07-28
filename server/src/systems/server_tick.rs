use crate::resources::{AppliedSeqs, PendingInputs, PlayerStates, ServerTickTimer};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::constants::{CH_S2C, MOVE_SPEED, TICK_DT};
use protocol::messages::{PlayerSnapshot, S2C, WorldSnapshot};

pub fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut server: ResMut<QuinnetServer>,
) {
    // ==== ДО построения снапшота ====
    {
        let list: Vec<u64> = states.0.keys().copied().collect();
        info!("🟢 PlayerStates BEFORE snapshot: {:?}", list);
    }

    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    for (&id, queue) in pending.0.iter_mut() {
        if let Some(last) = queue.back() {
            let st = states.0.entry(id).or_default();
            let mut dir = Vec2::ZERO;
            if last.up {
                dir.y += 1.0;
            }
            if last.down {
                dir.y -= 1.0;
            }
            if last.left {
                dir.x -= 1.0;
            }
            if last.right {
                dir.x += 1.0;
            }
            st.pos += dir.normalize_or_zero() * MOVE_SPEED * TICK_DT;
            st.rot = last.rotation;
            st.stance = last.stance.clone();
            applied.0.insert(id, last.seq);
        }
        queue.clear();
    }

    let snapshot = WorldSnapshot {
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
        last_input_seq: applied.0.clone(),
    };

    let ids: Vec<u64> = snapshot.players.iter().map(|p| p.id).collect();

    info!(
        "[Server] → Snapshot t={} players=[{}]",
        snapshot.server_time,
        snapshot
            .players
            .iter()
            .map(|p| p.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    server
        .endpoint_mut()
        .broadcast_message_on(CH_S2C, S2C::Snapshot(snapshot))
        .unwrap();

    // ==== ПОСЛЕ рассылки ====
    info!("📤 Snapshot sent ({} players): {:?}", ids.len(), ids);
}
