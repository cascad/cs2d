use crate::components::LocalPlayer;
use crate::resources::{CurrentStance, PendingInputsClient, SendTimer, SeqCounter, SolidTiles};
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_C2S, MOVE_SPEED, PLAYER_SIZE, TILE_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

/// Проверка коллизии через готовый набор занятых тайлов
fn is_blocked(pos: Vec2, solids: &SolidTiles) -> bool {
    let half = PLAYER_SIZE * 0.5;
    let min = pos + Vec2::new(-half, -half);
    let max = pos + Vec2::new(half, half);

    // Тайлы, которые занимает игрок
    let tx_min = (min.x / TILE_SIZE).floor() as i32;
    let tx_max = (max.x / TILE_SIZE).floor() as i32;
    let ty_min = (min.y / TILE_SIZE).floor() as i32;
    let ty_max = (max.y / TILE_SIZE).floor() as i32;

    for tx in tx_min..=tx_max {
        for ty in ty_min..=ty_max {
            if solids.0.contains(&IVec2::new(tx, ty)) {
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
    solids: Res<SolidTiles>,
    mut player_q: Query<&mut Transform, With<LocalPlayer>>,
) {
    // направление движения
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { dir.y += 1.; }
    if keys.pressed(KeyCode::KeyS) { dir.y -= 1.; }
    if keys.pressed(KeyCode::KeyA) { dir.x -= 1.; }
    if keys.pressed(KeyCode::KeyD) { dir.x += 1.; }
    dir = dir.normalize_or_zero();

    // движение и коллизия проверяются каждый кадр
    if let Ok(mut tf) = player_q.single_mut() {
        let current = tf.translation.truncate();
        let delta   = dir * MOVE_SPEED * TICK_DT;
        let mut new = current;

        let proposed_x = Vec2::new(current.x + delta.x, current.y);
        if !is_blocked(proposed_x, &solids) {
            new.x = proposed_x.x;
        }
        let proposed_y = Vec2::new(new.x, current.y + delta.y);
        if !is_blocked(proposed_y, &solids) {
            new.y = proposed_y.y;
        }

        tf.translation.x = new.x;
        tf.translation.y = new.y;

        // отправляем только раз в тик
        if timer.0.tick(time.delta()).just_finished() {
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
            client
                .connection_mut()
                .send_message_on(CH_C2S, C2S::Input(inp.clone()))
                .ok();
            pending.0.push_back(inp);
            if pending.0.len() > 256 {
                pending.0.pop_front();
            }
        }
    }
}
