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
    // Угловой допуск расширяем на видимый «радиус» цели: крупная/близкая цель
    // занимает больший угол, поэтому попасть по ней проще (меньше «промахов»).
    let ang_radius = (target_radius / dist).min(1.0).asin();
    let eff_half = (half_angle + ang_radius).min(core::f32::consts::PI);
    cos_to >= eff_half.cos()
}

/// Попадание удара-«капсулы»: зона ПОСТОЯННОЙ ширины `half_width` вдоль отрезка
/// от ЦЕНТРА атакующего на `range` вперёд по `facing_dir`. Цель радиуса
/// `target_radius` поражена, если её центр ближе `half_width + target_radius` к
/// этому отрезку. В отличие от конуса, ширина у самой модели такая же, как и
/// вдали — урон вблизи не теряется. Считаем строго от центра (не от края модели).
pub fn in_melee_swath(
    attacker: Vec2,
    facing_dir: Vec2,
    target: Vec2,
    target_radius: f32,
    range: f32,
    half_width: f32,
) -> bool {
    let dir = facing_dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return false;
    }
    let to = target - attacker;
    // проекция на направление взгляда (вдоль) и поперёк
    let along = to.dot(dir).clamp(0.0, range); // ближайшая точка отрезка [0..range]
    let closest = dir * along;
    let perp = (to - closest).length();
    perp <= half_width + target_radius
}

/// Поглощает ли активный блок цели входящий удар. Блок прикрывает ФРОНТ цели
/// (+ немного боков): источник урона `source` должен лежать в пределах полу-угла
/// `half_angle` (рад) от направления взгляда цели `facing` (рад). Удары сбоку и в
/// спину проходят. Вплотную — считаем попаданием в блок (спереди).
pub fn block_absorbs(target: Vec2, facing: f32, source: Vec2, half_angle: f32) -> bool {
    let to = source - target;
    let dist = to.length();
    if dist <= 1e-4 {
        return true;
    }
    let face = Vec2::new(facing.cos(), facing.sin());
    let cos_to = (to / dist).dot(face);
    cos_to >= half_angle.cos()
}

/// В поле зрения ли `target` для наблюдателя в `viewer`, смотрящего под углом
/// `facing` (рад). Поле зрения — КОНУС полу-угла `half_angle` (рад) вокруг
/// взгляда: как у человека есть боковое зрение, но за спиной — слепая зона.
/// Радиус/стены проверяются отдельно. Вплотную — считаем видимым.
#[inline]
pub fn in_fov(viewer: Vec2, facing: f32, target: Vec2, half_angle: f32) -> bool {
    // полный обзор (>=180° полу-угол) — конуса нет
    if half_angle >= core::f32::consts::PI {
        return true;
    }
    let to = target - viewer;
    let dist = to.length();
    if dist <= 1e-4 {
        return true;
    }
    let face = Vec2::new(facing.cos(), facing.sin());
    (to / dist).dot(face) >= half_angle.cos()
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

    #[test]
    fn swath_constant_width_hits_near_and_far() {
        let w = 30.0;
        // цель вплотную сбоку (вдоль=0, поперёк=25) — попадает (узкий конус бы промахнулся)
        assert!(in_melee_swath(Vec2::ZERO, dir_right(), Vec2::new(2.0, 25.0), 0.0, RANGE, w));
        // та же поперечная дистанция, но далеко вперёд — тоже попадает (ширина постоянна)
        assert!(in_melee_swath(Vec2::ZERO, dir_right(), Vec2::new(60.0, 25.0), 0.0, RANGE, w));
        // слишком вбок (поперёк > ширина+радиус) — мимо
        assert!(!in_melee_swath(Vec2::ZERO, dir_right(), Vec2::new(30.0, 70.0), 0.0, RANGE, w));
        // позади — мимо
        assert!(!in_melee_swath(Vec2::ZERO, dir_right(), Vec2::new(-40.0, 0.0), 0.0, RANGE, w));
        // дальше досягаемости — мимо
        assert!(!in_melee_swath(Vec2::ZERO, dir_right(), Vec2::new(RANGE + 60.0, 0.0), 0.0, RANGE, w));
    }

    #[test]
    fn fov_sees_front_and_sides_not_behind() {
        let half = 110f32.to_radians();
        let me = Vec2::ZERO;
        let facing = 0.0; // смотрю вправо (+X)
        assert!(in_fov(me, facing, Vec2::new(50.0, 0.0), half), "прямо перед — видно");
        assert!(in_fov(me, facing, Vec2::new(0.0, 50.0), half), "сбоку (90°) — видно (есть периферия)");
        assert!(!in_fov(me, facing, Vec2::new(-50.0, 0.0), half), "за спиной — НЕ видно");
        // 100° в сторону — внутри 110° → видно; 130° → уже нет
        let a = 130f32.to_radians();
        assert!(!in_fov(me, facing, Vec2::new(50.0 * a.cos(), 50.0 * a.sin()), half));
    }

    #[test]
    fn block_absorbs_front_not_back_or_side() {
        let half = 75f32.to_radians();
        let me = Vec2::ZERO;
        let facing = 0.0; // смотрю вправо (+X)
        // источник прямо спереди — блок держит
        assert!(block_absorbs(me, facing, Vec2::new(50.0, 0.0), half));
        // источник чуть сбоку, в пределах фронтального сектора — держит
        assert!(block_absorbs(me, facing, Vec2::new(40.0, 30.0), half));
        // источник строго сбоку (90°) — вне 75° → проходит
        assert!(!block_absorbs(me, facing, Vec2::new(0.0, 50.0), half));
        // источник со спины — проходит
        assert!(!block_absorbs(me, facing, Vec2::new(-50.0, 0.0), half));
    }
}
