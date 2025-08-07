use crate::components::LocalPlayer;
use crate::resources::{CurrentStance, PendingInputsClient, SendTimer, SeqCounter};
use crate::systems::level::Wall;
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::{CH_C2S, MOVE_SPEED, PLAYER_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

// pub fn send_input_and_predict(
//     keys: Res<ButtonInput<KeyCode>>,
//     time: Res<Time>,
//     mut timer: ResMut<SendTimer>,
//     mut client: ResMut<QuinnetClient>,
//     stance: Res<CurrentStance>,
//     mut seq: ResMut<SeqCounter>,
//     mut pending: ResMut<PendingInputsClient>,
//     mut q: Query<&mut Transform, With<LocalPlayer>>,
//     wall_query: Query<(&Transform, &Sprite), With<Wall>>,
// ) {
//     if !timer.0.tick(time.delta()).just_finished() {
//         return;
//     }

//     if let Ok(mut t) = q.single_mut() {
//         seq.0 = seq.0.wrapping_add(1);
//         let inp = InputState {
//             seq: seq.0,
//             up: keys.pressed(KeyCode::KeyW),
//             down: keys.pressed(KeyCode::KeyS),
//             left: keys.pressed(KeyCode::KeyA),
//             right: keys.pressed(KeyCode::KeyD),
//             rotation: t.rotation.to_euler(EulerRot::XYZ).2,
//             stance: stance.0.clone(),
//             timestamp: time_in_seconds(),
//         };
//         client
//             .connection_mut()
//             .send_message_on(CH_C2S, C2S::Input(inp.clone()))
//             .ok();
//         pending.0.push_back(inp.clone());
//         if pending.0.len() > 256 {
//             pending.0.pop_front();
//         }

//         // --- ПРЕДСКАЗАНИЕ С УЧЕТОМ КОЛЛИЗИЙ ---
//         // Вычисляем направление
//         let mut dir = Vec2::ZERO;
//         if inp.up {
//             dir.y += 1.0;
//         }
//         if inp.down {
//             dir.y -= 1.0;
//         }
//         if inp.left {
//             dir.x -= 1.0;
//         }
//         if inp.right {
//             dir.x += 1.0;
//         }
//         dir = dir.normalize_or_zero();

//         // Смещение по предсказанию
//         let delta = (dir * MOVE_SPEED * TICK_DT).extend(0.0);
//         let desired = t.translation + delta;

//         // Проверка коллизий со стенами
//         let mut blocked = false;
//         let player_pos = desired.truncate();
//         let player_size = Vec2::splat(PLAYER_SIZE);
//         for (wall_tf, sprite) in wall_query.iter() {
//             if let Some(wall_size) = sprite.custom_size {
//                 if aabb_collide(player_pos, player_size, wall_tf, wall_size) {
//                     blocked = true;
//                     break;
//                 }
//             }
//         }

//         if !blocked {
//             t.translation = desired;
//         }
//         // иначе оставляем старую позицию (откат)

//         // todo rm
//         simulate_input(&mut *t, &inp);
//     }
// }

// pub fn send_input_and_predict(
//     keys: Res<ButtonInput<KeyCode>>,
//     time: Res<Time>,
//     mut timer: ResMut<SendTimer>,
//     mut client: ResMut<QuinnetClient>,
//     stance: Res<CurrentStance>,
//     mut seq: ResMut<SeqCounter>,
//     mut pending: ResMut<PendingInputsClient>,
//     mut q: Query<&mut Transform, With<LocalPlayer>>,
//     wall_query: Query<(&Transform, &Sprite), With<Wall>>,
// ) {
//     if !timer.0.tick(time.delta()).just_finished() {
//         return;
//     }

//     if let Ok(mut t) = q.single_mut() {
//         seq.0 = seq.0.wrapping_add(1);
//         let inp = InputState {
//             seq: seq.0,
//             up: keys.pressed(KeyCode::KeyW),
//             down: keys.pressed(KeyCode::KeyS),
//             left: keys.pressed(KeyCode::KeyA),
//             right: keys.pressed(KeyCode::KeyD),
//             rotation: t.rotation.to_euler(EulerRot::XYZ).2,
//             stance: stance.0.clone(),
//             timestamp: time_in_seconds(),
//         };

//         // Отправляем на сервер
//         client
//             .connection_mut()
//             .send_message_on(CH_C2S, C2S::Input(inp.clone()))
//             .ok();

//         // Локальное буферизованное предсказание
//         pending.0.push_back(inp.clone());
//         if pending.0.len() > 256 {
//             pending.0.pop_front();
//         }

//         // Собираем список стен (Transform + size)
//         let walls: Vec<(&Transform, Vec2)> = wall_query
//             .iter()
//             .filter_map(|(wall_tf, sprite)| sprite.custom_size.map(|s| (wall_tf, s)))
//             .collect();

//         // Применяем инпут с коллизиями
//         simulate_input_with_collision(&mut *t, &inp, &walls);
//     }
// }

// Применяет инпут с предсказанием и коллизией. wall_iter — итератор по стенам (Transform, size).
// pub fn simulate_input_with_collision(
//     t: &mut Transform,
//     inp: &InputState,
//     walls: &[(&Transform, Vec2)],
// ) {
//     // направление
//     let mut dir = Vec2::ZERO;
//     if inp.up {
//         dir.y += 1.0;
//     }
//     if inp.down {
//         dir.y -= 1.0;
//     }
//     if inp.left {
//         dir.x -= 1.0;
//     }
//     if inp.right {
//         dir.x += 1.0;
//     }
//     dir = dir.normalize_or_zero();

//     // желаемое смещение
//     let delta = (dir * MOVE_SPEED * TICK_DT).extend(0.0);
//     let desired = t.translation + delta;

//     // проверка пересечения
//     let mut blocked = false;
//     let player_pos = desired.truncate();
//     let player_size = Vec2::splat(PLAYER_SIZE);
//     for (wall_tf, wall_size) in walls {
//         let colliding = aabb_collide(player_pos, player_size, wall_tf, *wall_size);
//         if colliding {
//             info!(
//                 "COLLISION block: player_pos={:?} player_size={:?} wall_pos={:?} wall_size={:?}",
//                 player_pos, player_size, wall_tf.translation, wall_size
//             );
//             blocked = true;
//             break;
//         }
//     }

//     if !blocked {
//         t.translation = desired;
//     }
//     // всегда обновляем ориентацию
//     t.rotation = Quat::from_rotation_z(inp.rotation);
// }

// fn aabb_collide(pos: Vec2, size: Vec2, wall_tf: &Transform, wall_size: Vec2) -> bool {
//     let half_p = size / 2.0;
//     let half_w = wall_size / 2.0;

//     let p_min = pos - half_p;
//     let p_max = pos + half_p;

//     let w_pos = wall_tf.translation.truncate();
//     let w_min = w_pos - half_w;
//     let w_max = w_pos + half_w;

//     !(p_max.x < w_min.x || p_min.x > w_max.x || p_max.y < w_min.y || p_min.y > w_max.y)
// }

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

    // Берём трансформ локального игрока (чтобы читать и мутировать)
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

        // Буфер предсказаний
        pending.0.push_back(inp.clone());
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }

        // Собираем список стен (Transform + size)
        let walls: Vec<(&Transform, Vec2)> = wall_q
            .iter()
            .filter_map(|(wall_tf, sprite)| sprite.custom_size.map(|s| (wall_tf, s)))
            .collect();

        // Применяем инпут с коллизией
        simulate_input_with_collision(&mut *t, &inp, &walls);
    }
}

// fn aabb_intersection(
//     pos: Vec2,
//     size: Vec2,
//     wall_tf: &Transform,
//     wall_size: Vec2,
// ) -> Option<Vec2> {
//     let half_p = size / 2.0;
//     let half_w = wall_size / 2.0;

//     let p_min = pos - half_p;
//     let p_max = pos + half_p;

//     let w_pos = wall_tf.translation.truncate();
//     let w_min = w_pos - half_w;
//     let w_max = w_pos + half_w;

//     // Проверка пересечения
//     if p_max.x < w_min.x || p_min.x > w_max.x || p_max.y < w_min.y || p_min.y > w_max.y {
//         return None;
//     }

//     // Вычисляем минимальный вектор выхода (MTV)
//     let overlap_x = if p_max.x - w_min.x < w_max.x - p_min.x {
//         p_max.x - w_min.x
//     } else {
//         w_max.x - p_min.x
//     };
//     let overlap_y = if p_max.y - w_min.y < w_max.y - p_min.y {
//         p_max.y - w_min.y
//     } else {
//         w_max.y - p_min.y
//     };

//     // Выбираем меньший по величине толчок
//     if overlap_x < overlap_y {
//         // Сдвигаем по X: направление зависит от того, с какой стороны проникновение
//         let correction = if (pos.x) > w_pos.x {
//             Vec2::new(overlap_x, 0.0)
//         } else {
//             Vec2::new(-overlap_x, 0.0)
//         };
//         Some(correction)
//     } else {
//         let correction = if (pos.y) > w_pos.y {
//             Vec2::new(0.0, overlap_y)
//         } else {
//             Vec2::new(0.0, -overlap_y)
//         };
//         Some(correction)
//     }
// }

// pub fn simulate_input_with_collision(
//     t: &mut Transform,
//     inp: &InputState,
//     walls: &[(&Transform, Vec2)],
// ) {
//     // направление
//     let mut dir = Vec2::ZERO;
//     if inp.up {
//         dir.y += 1.0;
//     }
//     if inp.down {
//         dir.y -= 1.0;
//     }
//     if inp.left {
//         dir.x -= 1.0;
//     }
//     if inp.right {
//         dir.x += 1.0;
//     }
//     dir = dir.normalize_or_zero();

//     let speed = MOVE_SPEED * TICK_DT;
//     let delta = dir * speed; // Vec2

//     let player_size = Vec2::splat(PLAYER_SIZE);

//     // Сохраняем исходную (непротолкнутую) позицию
//     let orig = t.translation;

//     // Попытка полного движения
//     let mut candidate = orig + delta.extend(0.0);

//     // Вспомогательная функция проверки перекрытия
//     let is_colliding = |pos: Vec3| {
//         let pos2d = pos.truncate();
//         for (wall_tf, wall_size) in walls.iter() {
//             if aabb_overlap(pos2d, player_size, wall_tf, *wall_size) {
//                 return true;
//             }
//         }
//         false
//     };

//     // Если полный вектор ведёт в коллизию — делим по осям
//     if is_colliding(candidate) {
//         candidate = orig; // откат

//         // Попробуем по X
//         let attempt_x = (orig + Vec3::new(delta.x, 0.0, 0.0));
//         if !is_colliding(attempt_x) {
//             candidate = attempt_x;
//         }

//         // Попробуем по Y (на основе того, что уже применили по X)
//         let attempt_y = Vec3::new(candidate.x, orig.y + delta.y, orig.z);
//         if !is_colliding(attempt_y) {
//             candidate = attempt_y;
//         }
//     }

//     // Наконец, применяем вычисленную позицию
//     t.translation = candidate;

//     // Поворот как обычно
//     t.rotation = Quat::from_rotation_z(inp.rotation);
// }

// // простая проверка перекрытия AABB (без коррекции)
// fn aabb_overlap(pos: Vec2, size: Vec2, wall_tf: &Transform, wall_size: Vec2) -> bool {
//     let half_p = size / 2.0;
//     let half_w = wall_size / 2.0;

//     let p_min = pos - half_p;
//     let p_max = pos + half_p;

//     let w_pos = wall_tf.translation.truncate();
//     let w_min = w_pos - half_w;
//     let w_max = w_pos + half_w;

//     !(p_max.x < w_min.x || p_min.x > w_max.x || p_max.y < w_min.y || p_min.y > w_max.y)
// }

fn segment_intersects_aabb(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> bool {
    // Liang-Barsky line vs AABB
    let d = p1 - p0;
    let mut tmin = 0.0_f32;
    let mut tmax = 1.0_f32;

    for i in 0..2 {
        let (p, q, r_min, r_max) = if i == 0 {
            (d.x, p0.x, min.x, max.x)
        } else {
            (d.y, p0.y, min.y, max.y)
        };

        if p.abs() < f32::EPSILON {
            if q < r_min || q > r_max {
                return false;
            }
        } else {
            let inv = 1.0 / p;
            let mut t1 = (r_min - q) * inv;
            let mut t2 = (r_max - q) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

fn point_in_aabb(p: Vec2, min: Vec2, max: Vec2) -> bool {
    p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y
}

/// Применяет инпут с предсказанием и устойчивой коллизией (swept + поосевая).
pub fn simulate_input_with_collision(
    t: &mut Transform,
    inp: &InputState,
    walls: &[(&Transform, Vec2)],
) {
    // направление
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

    let delta = dir * (MOVE_SPEED * TICK_DT); // Vec2
    let orig = t.translation.truncate();
    let mut new_pos = orig + delta;

    // Размеры для Minkowski: расширяем стены на половину игрока, а игрок точка
    let half = Vec2::splat(PLAYER_SIZE / 2.0);

    // Проверяем swept коллизию: путь от orig к new_pos
    let mut blocked_full = false;
    for (wall_tf, wall_size) in walls.iter() {
        let wall_center = wall_tf.translation.truncate();
        let half_wall = *wall_size / 2.0;
        let rect_min = wall_center - half_wall - half;
        let rect_max = wall_center + half_wall + half;

        // если сегмент пересекает расширенную AABB — полный вектор блокируем
        if segment_intersects_aabb(orig, new_pos, rect_min, rect_max)
            || point_in_aabb(new_pos, rect_min, rect_max)
        {
            blocked_full = true;
            break;
        }
    }

    let final_pos = if !blocked_full {
        new_pos
    } else {
        // поосевая разбивка: сначала X, потом Y
        let mut candidate = orig;

        // X
        let attempt_x = orig + Vec2::new(delta.x, 0.0);
        let mut blocked_x = false;
        for (wall_tf, wall_size) in walls.iter() {
            let wall_center = wall_tf.translation.truncate();
            let half_wall = *wall_size / 2.0;
            let rect_min = wall_center - half_wall - half;
            let rect_max = wall_center + half_wall + half;

            if segment_intersects_aabb(orig, attempt_x, rect_min, rect_max)
                || point_in_aabb(attempt_x, rect_min, rect_max)
            {
                blocked_x = true;
                break;
            }
        }
        if !blocked_x {
            candidate.x += delta.x;
        }

        // Y (от текущего candidate.x)
        let attempt_y = Vec2::new(candidate.x, orig.y + delta.y);
        let mut blocked_y = false;
        for (wall_tf, wall_size) in walls.iter() {
            let wall_center = wall_tf.translation.truncate();
            let half_wall = *wall_size / 2.0;
            let rect_min = wall_center - half_wall - half;
            let rect_max = wall_center + half_wall + half;

            if segment_intersects_aabb(orig, attempt_y, rect_min, rect_max)
                || point_in_aabb(attempt_y, rect_min, rect_max)
            {
                blocked_y = true;
                break;
            }
        }
        if !blocked_y {
            candidate.y += delta.y;
        }

        candidate
    };

    t.translation.x = final_pos.x;
    t.translation.y = final_pos.y;
    t.rotation = Quat::from_rotation_z(inp.rotation);
}