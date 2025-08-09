use crate::constants::{HITBOX_RADIUS, MAX_RAY_LEN};
use crate::resources::PlayerState;
use crate::systems::wall::Wall;
use bevy::prelude::*;
use protocol::messages::ShootEvent;
use std::collections::{HashMap, VecDeque};

/// Сохраняем историю состояний
pub fn push_history(
    history: &mut crate::resources::SnapshotHistory,
    now: f64,
    states: &HashMap<u64, PlayerState>,
) {
    history.buf.push_back((now, states.clone()));
    if history.buf.len() > history.cap {
        history.buf.pop_front();
    }
}

// --- ВАША ФУНКЦИЯ С ДОБАВЛЕННЫМ LOS ---
pub fn check_hit_lag_comp(
    history: &std::collections::VecDeque<(f64, std::collections::HashMap<u64, PlayerState>)>,
    current: &std::collections::HashMap<u64, PlayerState>,
    shoot: &ShootEvent,
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,        // ← НОВЫЙ ПАРАМЕТР
) -> Option<u64> {
    // Находим два снапшота вокруг shoot.timestamp
    let mut prev = None;
    let mut next = None;
    for (t, states) in history {
        if *t <= shoot.timestamp {
            prev = Some((t, states));
        } else if next.is_none() {
            next = Some((t, states));
        }
    }
    let (t0, s0, t1, s1) = match (prev, next) {
        (Some((t0, s0)), Some((t1, s1))) => (*t0, s0, *t1, s1),
        (Some((t0, s0)), None) => (*t0, s0, *t0, s0),
        _ => return None,
    };
    let alpha = ((shoot.timestamp - t0) / (t1 - t0).max(1e-4)).clamp(0.0, 1.0) as f32;

    // интерполируем все позиции
    let mut interp: std::collections::HashMap<u64, PlayerState> = std::collections::HashMap::new();
    for (&id, p0) in s0.iter() {
        if let Some(p1) = s1.get(&id) {
            let lerped_pos = p0.pos.lerp(p1.pos, alpha);
            let lerped_rot = p0.rot + (p1.rot - p0.rot) * alpha;
            interp.insert(
                id,
                PlayerState {
                    pos: lerped_pos,
                    rot: lerped_rot,
                    stance: p1.stance.clone(),
                    hp: p1.hp,
                },
            );
        }
    }

    // луч из позиции стрелка (в интерполированном снапе)
    let shooter = interp.get(&shoot.shooter_id)?;
    let dir = shoot.dir.normalize_or_zero();

    for (&id, target) in interp.iter() {
        if id == shoot.shooter_id {
            continue;
        }
        let to_target = target.pos - shooter.pos;
        // скалярная проекция на нормализованный dir
        let proj_len = to_target.dot(dir);
        if proj_len < 0.0 { continue; } // позади
        if proj_len > MAX_RAY_LEN { continue; }

        // ближайшая точка луча к центру цели
        let nearest = shooter.pos + dir * proj_len;

        // радиальное расстояние от центра цели до луча
        if to_target.length() > 0.0 && to_target.distance(dir * proj_len) <= HITBOX_RADIUS {
            // NEW: проверяем видимость до ближайшей точки попадания (а не до центра)
            if los_blocked_by_walls(shooter.pos, nearest, wall_q) {
                continue; // стена закрывает — не считаем попаданием
            }
            return Some(id);
        }
    }
    None
}

fn los_blocked_by_walls(
    p0: Vec2,
    p1: Vec2,
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,
) -> bool {
    let eps = 0.001;
    for (wt, sprite) in wall_q.iter() {
        if let Some(size) = sprite.custom_size {
            let half = size / 2.0 - Vec2::splat(eps);
            let c = wt.translation.truncate();
            let min = c - half;
            let max = c + half;
            if segment_aabb_intersect_t(p0, p1, min, max).is_some() {
                return true;
            }
        }
    }
    false
}

// ---------- Утилита: пересечение отрезка (луча) с AABB стены ----------
#[inline]
fn segment_aabb_intersect_t(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> Option<f32> {
    // Liang–Barsky / slab
    let d = p1 - p0;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;

    if d.x.abs() < f32::EPSILON {
        if p0.x < min.x || p0.x > max.x {
            return None;
        }
    } else {
        let inv = 1.0 / d.x;
        let mut tmin = (min.x - p0.x) * inv;
        let mut tmax = (max.x - p0.x) * inv;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }

    if d.y.abs() < f32::EPSILON {
        if p0.y < min.y || p0.y > max.y {
            return None;
        }
    } else {
        let inv = 1.0 / d.y;
        let mut tmin = (min.y - p0.y) * inv;
        let mut tmax = (max.y - p0.y) * inv;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0.clamp(0.0, 1.0))
}
