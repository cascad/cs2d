use crate::{
    resources::{AppliedSeqs, PendingInputs, PlayerStates, ServerTickTimer, SnapshotHistory},
    utils::push_history,
};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{CH_S2C, MOVE_SPEED, TICK_DT},
    messages::{PlayerSnapshot, S2C, WorldSnapshot},
};

pub fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut history: ResMut<SnapshotHistory>, // ← добавили
    mut server: ResMut<QuinnetServer>,
) {
    // 1) Ждём конца тика
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    // 2) Применяем все входы
    for (&id, queue) in pending.0.iter_mut() {
        if let Some(input) = queue.back() {
            let st = states.0.entry(id).or_default();
            let mut dir = Vec2::ZERO;
            if input.up {
                dir.y += 1.0;
            }
            if input.down {
                dir.y -= 1.0;
            }
            if input.left {
                dir.x -= 1.0;
            }
            if input.right {
                dir.x += 1.0;
            }
            st.pos += dir.normalize_or_zero() * MOVE_SPEED * TICK_DT;
            st.rot = input.rotation;
            st.stance = input.stance.clone();
            applied.0.insert(id, input.seq);
        }
        queue.clear();
    }

    // 3) Собираем снапшот
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

    // 4) Рассылаем всем
    server
        .endpoint_mut()
        .broadcast_message_on(CH_S2C, S2C::Snapshot(snapshot.clone()))
        .unwrap();

    // 5) Записываем в историю для лаг‑компенсации
    push_history(&mut history, snapshot.server_time, &states.0);
}
