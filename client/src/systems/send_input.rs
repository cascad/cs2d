use crate::components::LocalPlayer;
use crate::resources::{CurrentStance, PendingInputsClient, SendTimer, SeqCounter};
use crate::systems::utils::time_in_seconds;
use crate::systems::level::Wall;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_C2S, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

/// AABB-попадание: пересечение двух прямоугольников
fn aabb_intersect(min_a: Vec2, max_a: Vec2, min_b: Vec2, max_b: Vec2) -> bool {
    !(max_a.x < min_b.x || min_a.x > max_b.x || max_a.y < min_b.y || min_a.y > max_b.y)
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
    wall_q: Query<(&Transform, &Sprite), (With<Wall>, Without<LocalPlayer>)>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    // направление
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir.x += 1.0;
    }
    dir = dir.normalize_or_zero();

    if let Ok(mut tf) = player_q.single_mut() {
        let current = tf.translation.truncate();
        let delta = dir * (MOVE_SPEED * TICK_DT);
        let proposed = current + delta;

        // AABB игрока в предложенной позиции
        let half = PLAYER_SIZE * 0.5;
        let player_min = proposed + Vec2::new(-half, -half);
        let player_max = proposed + Vec2::new(half, half);

        // проверка столкновений со стенами
        let mut blocked = false;
        for (wall_tf, sprite) in wall_q.iter() {
            if let Some(size) = sprite.custom_size {
                let half_wall = size / 2.0;
                let wall_center = wall_tf.translation.truncate();
                let wall_min = wall_center - half_wall;
                let wall_max = wall_center + half_wall;

                if aabb_intersect(player_min, player_max, wall_min, wall_max) {
                    blocked = true;
                    break;
                }
            }
        }

        if !blocked {
            tf.translation.x = proposed.x;
            tf.translation.y = proposed.y;
        } else {
            info!(
                target: "collision",
                "movement blocked by wall: from {:?} to {:?}",
                current, proposed
            );
        }

        // формируем input
        let rotation = tf.rotation.to_euler(EulerRot::XYZ).2;
        seq.0 = seq.0.wrapping_add(1);
        let inp = InputState {
            seq: seq.0,
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
            rotation,
            stance: stance.0.clone(),
            timestamp: time_in_seconds(),
        };

        // отправка на сервер
        let _ = client
            .connection_mut()
            .send_message_on(CH_C2S, C2S::Input(inp.clone()));

        // pending
        pending.0.push_back(inp);
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }
    }
}
