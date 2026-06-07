//! Геометрия ближнего боя: попадает ли цель в сектор перед атакующим.

use glam::Vec2;

/// Лежит ли точка `target` в секторе удара атакующего:
/// - в пределах `range` (+ радиус хитбокса цели `target_radius`);
/// - в пределах полу-угла `half_angle` (рад) от направления `facing_dir`.
///
/// `facing_dir` не обязательно нормализован. Если цель практически совпадает с
/// атакующим — считаем попаданием.
pub fn in_melee_arc(
    attacker: Vec2,
    facing_dir: Vec2,
    target: Vec2,
    target_radius: f32,
    range: f32,
    half_angle: f32,
) -> bool {
    let to = target - attacker;
    let dist = to.length();
    if dist <= 1e-4 {
        return true; // вплотную
    }
    if dist > range + target_radius {
        return false;
    }
    let dir = facing_dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return false;
    }
    let cos_to = (to / dist).dot(dir);
    cos_to >= half_angle.cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: f32 = 64.0;
    const HALF: f32 = 0.6; // ~34°
    const R: f32 = 20.0;

    fn dir_right() -> Vec2 {
        Vec2::new(1.0, 0.0)
    }

    #[test]
    fn target_straight_ahead_in_range_hits() {
        assert!(in_melee_arc(Vec2::ZERO, dir_right(), Vec2::new(50.0, 0.0), R, RANGE, HALF));
    }

    #[test]
    fn target_behind_misses() {
        assert!(!in_melee_arc(Vec2::ZERO, dir_right(), Vec2::new(-50.0, 0.0), R, RANGE, HALF));
    }

    #[test]
    fn target_too_far_misses() {
        assert!(!in_melee_arc(Vec2::ZERO, dir_right(), Vec2::new(200.0, 0.0), R, RANGE, HALF));
    }

    #[test]
    fn target_slightly_off_axis_within_angle_hits() {
        // ~20° в сторону при дистанции 50 — в пределах 34°
        let t = Vec2::new(50.0 * (0.35f32).cos(), 50.0 * (0.35f32).sin());
        assert!(in_melee_arc(Vec2::ZERO, dir_right(), t, R, RANGE, HALF));
    }

    #[test]
    fn target_wide_off_axis_misses() {
        // 80° в сторону — вне сектора
        let a = 80f32.to_radians();
        let t = Vec2::new(50.0 * a.cos(), 50.0 * a.sin());
        assert!(!in_melee_arc(Vec2::ZERO, dir_right(), t, R, RANGE, HALF));
    }

    #[test]
    fn target_just_beyond_range_but_within_radius_hits() {
        // центр на 80 (>64), но радиус хитбокса 20 → 80 <= 64+20
        assert!(in_melee_arc(Vec2::ZERO, dir_right(), Vec2::new(80.0, 0.0), R, RANGE, HALF));
    }
}
