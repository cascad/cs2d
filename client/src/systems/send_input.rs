use crate::components::LocalPlayer;
use crate::resources::{CurrentStance, PendingInputsClient, SendTimer, SeqCounter};
use crate::systems::utils::time_in_seconds;
use crate::systems::level::Wall;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_C2S, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

fn aabb_intersect(min_a: Vec2, max_a: Vec2, min_b: Vec2, max_b: Vec2) -> bool {
    !(max_a.x < min_b.x || min_a.x > max_b.x || max_a.y < min_b.y || min_a.y > max_b.y)
}

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

pub fn send_input_and_predict(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
    stance: Res<CurrentStance>,
    mut seq: ResMut<SeqCounter>,
    mut pending: ResMut<PendingInputsClient>,
    mut player_q: Query<&mut Transform, (With<LocalPlayer>, Without<Wall>)>,
    wall_q: Query<(&Transform, &Sprite), With<Wall>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { dir.y += 1.; }
    if keys.pressed(KeyCode::KeyS) { dir.y -= 1.; }
    if keys.pressed(KeyCode::KeyA) { dir.x -= 1.; }
    if keys.pressed(KeyCode::KeyD) { dir.x += 1.; }
    dir = dir.normalize_or_zero();

    if let Ok(mut tf) = player_q.single_mut() {
        let current = tf.translation.truncate();
        let delta   = dir * MOVE_SPEED * TICK_DT;
        let mut new = current;

        let proposed_x = Vec2::new(current.x + delta.x, current.y);
        if !is_blocked(proposed_x, &wall_q) {
            new.x = proposed_x.x;
        }
        let proposed_y = Vec2::new(new.x, current.y + delta.y);
        if !is_blocked(proposed_y, &wall_q) {
            new.y = proposed_y.y;
        }
        tf.translation.x = new.x;
        tf.translation.y = new.y;

        seq.0 = seq.0.wrapping_add(1);
        let inp = InputState {
            seq: seq.0,
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
            rotation: tf.rotation.to_euler(EulerRot::XYZ).2,
            stance: stance.0.clone(),
            timestamp: time_in_seconds(),
        };
        client.connection_mut().send_message_on(CH_C2S, C2S::Input(inp.clone())).ok();
        pending.0.push_back(inp);
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }
    }
}
