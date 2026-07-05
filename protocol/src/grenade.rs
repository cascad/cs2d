//! Чистая физика гранаты/банки: один тик полёта с воздушным затуханием и
//! отскоком от стен (AABB). Без ECS/сети — общий код для сервера (`bevy_quinnet`)
//! и нового стека на Lightyear (`netproto`), плюс golden-тесты траектории.
//!
//! «Якорное» представление: `pos(t) = from + dir * speed * elapsed`, где `elapsed`
//! — время с момента создания на начало тика. Поля обновляются каждый тик; таймер
//! взрыва (созд. время) живёт снаружи.

use crate::constants::{
    GRENADE_AIR_DRAG_PER_SEC, GRENADE_BLAST_RADIUS, GRENADE_BOUNCE_DAMPING, GRENADE_DAMAGE_COEFF,
    GRENADE_RADIUS, GRENADE_RESTITUTION, GRENADE_STOP_SPEED, MAX_STEP, SEPARATION_EPS,
};
use glam::Vec2;

/// Урон по дистанции от эпицентра: линейно спадает от центра к краю радиуса.
#[inline]
pub fn blast_damage(dist: f32) -> i32 {
    let falloff = ((GRENADE_BLAST_RADIUS - dist) / GRENADE_BLAST_RADIUS).clamp(0.0, 1.0);
    ((falloff * 50.0) * GRENADE_DAMAGE_COEFF) as i32
}

/// Физическое состояние гранаты в «якорном» представлении.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrenadePhys {
    pub from: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    pub elapsed: f32,
}

/// Результат шага: новое состояние, текущая позиция, был ли отскок и был ли
/// вообще контакт со стеной (`hit_wall`).
#[derive(Clone, Copy, Debug)]
pub struct GrenadeStepResult {
    pub state: GrenadePhys,
    pub pos: Vec2,
    pub bounced: bool,
    pub hit_wall: bool,
}

/// Один тик физики гранаты: полёт с воздушным затуханием и отскоком от стен.
/// Чистая функция. `can_bounce`: `true` — отражаемся, `false` — останавливаемся
/// в точке контакта (граната разобьётся снаружи). `walls` — список AABB (min,max).
pub fn step_grenade(
    s: &GrenadePhys,
    dt: f32,
    walls: &[(Vec2, Vec2)],
    can_bounce: bool,
) -> GrenadeStepResult {
    let t = s.elapsed;
    let mut pos = s.from + s.dir * s.speed * t;

    if let Some((_n0, corr0)) = collide_circle_with_walls(pos, GRENADE_RADIUS, walls) {
        pos += corr0;
    }

    let mut speed = s.speed * (1.0 - GRENADE_AIR_DRAG_PER_SEC).powf(dt);
    if speed < GRENADE_STOP_SPEED {
        speed = 0.0;
    }

    let mut dir = s.dir;
    let mut remaining = dir * speed * dt;
    let mut bounced = false;
    let mut hit_wall = false;

    while remaining.length_squared() > 0.0 {
        let step_len = remaining.length().min(MAX_STEP);
        let step_dir = remaining.normalize_or_zero();
        let step = step_dir * step_len;
        let proposed = pos + step;

        if let Some((normal, corr)) = collide_circle_with_walls(proposed, GRENADE_RADIUS, walls) {
            pos = proposed + corr;
            hit_wall = true;

            if can_bounce {
                let v = dir * speed;
                let reflected = v - 2.0 * v.dot(normal) * normal;
                let new_speed = reflected.length() * GRENADE_RESTITUTION * GRENADE_BOUNCE_DAMPING;
                dir = if new_speed > 0.0 { reflected / new_speed } else { dir };
                speed = if new_speed < GRENADE_STOP_SPEED { 0.0 } else { new_speed };
                bounced = true;
            } else {
                speed = 0.0;
            }
            break;
        } else {
            pos = proposed;
            remaining -= step;
        }
    }

    GrenadeStepResult {
        state: GrenadePhys {
            // Якорь пересчитывается с НОВЫМ elapsed (t + dt): реконструкция
            // pos = from + dir·speed·elapsed на следующем тике обязана вернуть
            // ровно текущую позицию. Со старым `t` реконструкция сама двигала
            // гранату на speed·dt, ПЛЮС шаговый обход добавлял столько же —
            // двойная интеграция: полёт в 2× скорости и детонация на половине
            // дистанции (травел-лимит набирался вдвое быстрее фактического пути).
            from: pos - dir * speed * (t + dt),
            dir,
            speed,
            elapsed: t + dt,
        },
        pos,
        bounced,
        hit_wall,
    }
}

#[inline]
fn clamp_vec2(p: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
}

/// Точная коллизия круг (центр `c`, радиус `r`) против AABB стены. Возвращает
/// (наружная нормаль, минимальная коррекция центра, чтобы выйти из пересечения).
fn collide_circle_with_wall_precise(
    c: Vec2,
    r: f32,
    min_b: Vec2,
    max_b: Vec2,
) -> Option<(Vec2, Vec2)> {
    let closest = clamp_vec2(c, min_b, max_b);
    let delta = c - closest;
    let d2 = delta.length_squared();
    if d2 > r * r {
        return None;
    }
    if d2 > 0.0 {
        let dist = d2.sqrt();
        let n = delta / dist;
        let push = (r - dist) + SEPARATION_EPS;
        return Some((n, n * push));
    }
    let pen_left = (c.x - min_b.x).abs();
    let pen_right = (max_b.x - c.x).abs();
    let pen_bottom = (c.y - min_b.y).abs();
    let pen_top = (max_b.y - c.y).abs();
    let (n, push) = {
        let min_x = pen_left.min(pen_right);
        let min_y = pen_bottom.min(pen_top);
        if min_x < min_y {
            if pen_left < pen_right {
                (Vec2::NEG_X, r + SEPARATION_EPS)
            } else {
                (Vec2::X, r + SEPARATION_EPS)
            }
        } else if pen_bottom < pen_top {
            (Vec2::NEG_Y, r + SEPARATION_EPS)
        } else {
            (Vec2::Y, r + SEPARATION_EPS)
        }
    };
    Some((n, n * push))
}

/// Первая найденная коллизия круга со стенами.
pub fn collide_circle_with_walls(
    center: Vec2,
    r: f32,
    walls: &[(Vec2, Vec2)],
) -> Option<(Vec2, Vec2)> {
    for &(min_b, max_b) in walls {
        if let Some(hit) = collide_circle_with_wall_precise(center, r, min_b, max_b) {
            return Some(hit);
        }
    }
    None
}
