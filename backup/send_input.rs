use crate::components::LocalPlayer;
use crate::resources::{CurrentStance, PendingInputsClient, SendTimer, SeqCounter};
use crate::systems::level::Wall;
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_C2S, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{InputState, C2S};

/// Проверка попадания точки (игрок как точка) в расширенную стену (Minkowski: стена + половина игрока)
fn point_vs_wall_overlap(point: Vec2, wall_tf: &Transform, wall_size: Vec2) -> bool {
    let half_wall = wall_size / 2.0;
    let wall_center = wall_tf.translation.truncate();
    let half_player = Vec2::splat(PLAYER_SIZE / 2.0);
    let min = wall_center - half_wall - half_player;
    let max = wall_center + half_wall + half_player;
    point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
}

/// Разбивает движение на шаги и применяет поэтапно, чтобы не “проскочить” через стену.
fn apply_movement_with_step_collision(
    t: &mut Transform,
    delta: Vec2,
    walls: &[(&Transform, Vec2)],
) {
    let mut remaining = delta;
    let max_step = PLAYER_SIZE * 0.5; // не больше половины хитбокса за итерацию
    let mut pos = t.translation.truncate();

    while remaining.length_squared() > 0.0 {
        let step = if remaining.length() > max_step {
            remaining.normalize() * max_step
        } else {
            remaining
        };
        let attempt = pos + step;

        info!("  [STEP] remaining={:?} pos={:?}", remaining, pos);

        // проверка, что точка не в любой расширенной стене
        let mut blocked = false;
        for (wall_tf, wall_size) in walls.iter() {
            if point_vs_wall_overlap(attempt, wall_tf, *wall_size) {
                blocked = true;
                info!(
                    "    [STEP] blocked by wall at {:?} size {:?}",
                    wall_tf.translation, wall_size
                );
                break;
            }
        }

        if blocked {
            break; // дальше не идём
        } else {
            pos = attempt;
            remaining -= step;
        }
    }

    t.translation.x = pos.x;
    t.translation.y = pos.y;
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

    if let Ok(mut t) = player_q.single_mut() {
        seq.0 = seq.0.wrapping_add(1);
        let inp = InputState {
            seq: seq.0,
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
            rotation: t.rotation.to_euler(EulerRot::XYZ).2,
            stance: stance.0.clone(),
            timestamp: time_in_seconds(),
        };

        // Отправка на сервер
        client
            .connection_mut()
            .send_message_on(CH_C2S, C2S::Input(inp.clone()))
            .ok();

        // Локальный буфер входов
        pending.0.push_back(inp.clone());
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }

        // Собираем массив стен (Transform + размер)
        let walls: Vec<(&Transform, Vec2)> = wall_q
            .iter()
            .filter_map(|(wall_tf, sprite)| sprite.custom_size.map(|s| (wall_tf, s)))
            .collect();

        // Вычисляем направление и дельту
        let mut dir = Vec2::ZERO;
        if inp.up {
            dir.y += 1.0;
        }
        if inp.down {
            dir.y -= 1.0;
        }
        if inp.left {
            dir.x -= 1.0;
        }
        if inp.right {
            dir.x += 1.0;
        }
        dir = dir.normalize_or_zero();
        let delta = dir * (MOVE_SPEED * TICK_DT);

        // todo debug
        let player_pos_before = t.translation.truncate();
        info!(
            "[COLLISION DEBUG] delta={:?} orig={:?}, PLAYER_SIZE={}, walls={}",
            delta,
            player_pos_before,
            PLAYER_SIZE,
            walls.len()
        );
        for (wall_tf, wall_size) in walls.iter() {
            let wall_center = wall_tf.translation.truncate();
            info!(
                "[COLLISION DEBUG] wall at {:?} size {:?} expanded min/max = {:?} / {:?}",
                wall_center,
                wall_size,
                wall_center - *wall_size / 2.0 - Vec2::splat(PLAYER_SIZE / 2.0),
                wall_center + *wall_size / 2.0 + Vec2::splat(PLAYER_SIZE / 2.0),
            );
        }
        // end debug

        // Применяем движение с устойчивой коллизией
        apply_movement_with_step_collision(&mut *t, delta, &walls);

        // Обновляем поворот
        t.rotation = Quat::from_rotation_z(inp.rotation);
    }
}
