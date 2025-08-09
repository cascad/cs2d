use bevy::prelude::*;
use protocol::constants::{MOVE_SPEED, TICK_DT};
use protocol::messages::{InputState, Stance};

use crate::systems::level::Wall;

pub fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    now.as_secs_f64()
}

pub fn stance_color(s: &Stance) -> Color {
    match s {
        Stance::Standing => Color::srgb(0.20, 1.00, 0.20),
        Stance::Crouching => Color::srgb(0.15, 0.85, 1.00),
        Stance::Prone => Color::srgb(0.00, 0.60, 0.60),
    }
}

pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}

pub fn spawn_hp_ui(commands: &mut Commands, player_id: u64, hp: u32, font: Handle<Font>) -> Entity {
    commands
        .spawn((
            Text2d(format!("{} HP", hp)),
            TextFont {
                font: font.into(),
                font_size: 14.0,
                ..Default::default()
            },
            TextColor(Color::WHITE.into()),
        ))
        .id()
}

// ---------- Утилита: пересечение отрезка (луча) с AABB стены ----------
#[inline]
pub fn segment_aabb_intersect_t(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> Option<f32> {
    // Liang–Barsky / slab method, возвращаем t∈[0,1] ближайшего входа
    let d = p1 - p0;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;

    // X
    if d.x.abs() < f32::EPSILON {
        if p0.x < min.x || p0.x > max.x {
            return None;
        }
    } else {
        let inv_dx = 1.0 / d.x;
        let mut tmin = (min.x - p0.x) * inv_dx;
        let mut tmax = (max.x - p0.x) * inv_dx;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }
    // Y
    if d.y.abs() < f32::EPSILON {
        if p0.y < min.y || p0.y > max.y {
            return None;
        }
    } else {
        let inv_dy = 1.0 / d.y;
        let mut tmin = (min.y - p0.y) * inv_dy;
        let mut tmax = (max.y - p0.y) * inv_dy;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }

    // ближайшее вхождение
    Some(t0.clamp(0.0, 1.0))
}

// ---------- Рейкаст до ближайшей стены по направлению dir ----------
pub fn raycast_to_walls(
    origin: Vec2,
    dir: Vec2,
    max_dist: f32,
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,
) -> f32 {
    let eps = 0.001;
    let end = origin + dir * max_dist;
    let mut best_t: Option<f32> = None;

    for (wt, sprite) in wall_q.iter() {
        if let Some(size) = sprite.custom_size {
            // слегка «сжимаем» AABB, чтобы луч, лежащий ровно на грани, не давал ложных пересечений
            let half = size / 2.0 - Vec2::splat(eps);
            let c = wt.translation.truncate();
            let min = c - half;
            let max = c + half;
            if let Some(t) = segment_aabb_intersect_t(origin, end, min, max) {
                best_t = Some(best_t.map_or(t, |old| old.min(t)));
            }
        }
    }
    if let Some(t) = best_t {
        max_dist * t
    } else {
        max_dist
    }
}

pub fn raycast_to_walls_cached(
    origin: Vec2,
    dir: Vec2,
    max_dist: f32,
    cache: &[(Vec2, Vec2)],
) -> f32 {
    let end = origin + dir * max_dist;
    let mut best: Option<f32> = None;

    #[inline]
    fn segment_aabb_intersect_t(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> Option<f32> {
        let d = p1 - p0;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        if d.x.abs() < f32::EPSILON {
            if p0.x < min.x || p0.x > max.x {
                return None;
            }
        } else {
            let inv = 1.0 / d.x;
            let (mut tmin, mut tmax) = ((min.x - p0.x) * inv, (max.x - p0.x) * inv);
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
            let (mut tmin, mut tmax) = ((min.y - p0.y) * inv, (max.y - p0.y) * inv);
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

    for &(min, max) in cache {
        if let Some(t) = segment_aabb_intersect_t(origin, end, min, max) {
            best = Some(best.map_or(t, |b| b.min(t)));
        }
    }
    best.map(|t| t * max_dist).unwrap_or(max_dist)
}
