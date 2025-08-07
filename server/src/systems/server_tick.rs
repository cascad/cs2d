use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{CH_S2C, MOVE_SPEED, PLAYER_SIZE, TICK_DT},
    messages::{PlayerSnapshot, WorldSnapshot, S2C},
};
use crate::{
    resources::{AppliedSeqs, PendingInputs, PlayerStates, ServerTickTimer, SnapshotHistory}, systems::wall::Wall, utils::push_history
};

/// AABB intersection test between two rectangles
fn aabb_intersect(min_a: Vec2, max_a: Vec2, min_b: Vec2, max_b: Vec2) -> bool {
    !(max_a.x < min_b.x || min_a.x > max_b.x || max_a.y < min_b.y || min_a.y > max_b.y)
}

/// checks if position collides with any wall
fn is_blocked(pos: Vec2, wall_q: &Query<(&Transform, &Sprite), With<Wall>>) -> bool {
    let half = PLAYER_SIZE * 0.5;
    let min_a = pos + Vec2::new(-half, -half);
    let max_a = pos + Vec2::new( half,  half);
    for (wt, sprite) in wall_q.iter() {
        if let Some(size) = sprite.custom_size {
            let half_w = size / 2.0;
            let center = wt.translation.truncate();
            let min_b  = center - half_w;
            let max_b  = center + half_w;
            if aabb_intersect(min_a, max_a, min_b, max_b) {
                return true;
            }
        }
    }
    false
}

/// Server tick: applies pending inputs with sliding wall collision, broadcasts snapshot, and records history
pub fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut history: ResMut<SnapshotHistory>,
    mut server: ResMut<QuinnetServer>,
    wall_q: Query<(&Transform, &Sprite), With<Wall>>,  // walls for collision
) {
    // 1) Wait for tick end
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    // 2) Apply all inputs with sliding collision
    for (&id, queue) in pending.0.iter_mut() {
        if let Some(input) = queue.back() {
            let st = states.0.entry(id).or_default();
            // compute direction
            let mut dir = Vec2::ZERO;
            if input.up    { dir.y += 1.; }
            if input.down  { dir.y -= 1.; }
            if input.left  { dir.x -= 1.; }
            if input.right { dir.x += 1.; }
            dir = dir.normalize_or_zero();

            let current = st.pos;
            let delta   = dir * MOVE_SPEED * TICK_DT;
            // sliding: X then Y
            let mut new = current;
            let tx = new + Vec2::new(delta.x, 0.);
            if !is_blocked(tx, &wall_q) { new.x = tx.x; }
            let ty = new + Vec2::new(0., delta.y);
            if !is_blocked(ty, &wall_q) { new.y = ty.y; }
            st.pos = new;

            // store rotation, stance, seq
            st.rot    = input.rotation;
            st.stance = input.stance.clone();
            applied.0.insert(id, input.seq);
        }
        queue.clear();
    }

    // 3) Build snapshot
    let snapshot = WorldSnapshot {
        players: states.0.iter().map(|(&id, st)| PlayerSnapshot {
            id,
            x: st.pos.x,
            y: st.pos.y,
            rotation: st.rot,
            stance: st.stance.clone(),
            hp: st.hp,
        }).collect(),
        server_time: time.elapsed_secs_f64(),
        last_input_seq: applied.0.clone(),
    };

    // 4) Broadcast
    server.endpoint_mut()
        .broadcast_message_on(CH_S2C, S2C::Snapshot(snapshot.clone()))
        .unwrap();

    // 5) History
    push_history(&mut history, snapshot.server_time, &states.0);
}
