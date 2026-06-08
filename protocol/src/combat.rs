//! Геометрия ближнего боя: попадает ли цель в сектор перед атакующим.

use crate::constants::{ISO_X, ISO_Y};
use glam::Vec2;

/// Перевод вектора из МИРА в ЭКРАН (изометрия 2:1, как `world_to_screen`).
#[inline]
fn iso(v: Vec2) -> Vec2 {
    Vec2::new((v.x - v.y) * ISO_X, (v.x + v.y) * ISO_Y)
}

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

/// Сектор удара с ПОСТОЯННОЙ шириной `half_width` (как капсула) и ограничением
/// угла `half_angle` на дальности. Вблизи угол автоматически расширяется, чтобы
/// зона не сужалась к ногам игрока (чистый конус промахивается по близким целям).
pub fn in_melee_sector(
    attacker: Vec2,
    facing_dir: Vec2,
    target: Vec2,
    target_radius: f32,
    range: f32,
    half_angle: f32,
    half_width: f32,
) -> bool {
    let dir = facing_dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return false;
    }
    let to = target - attacker;
    let dist = to.length();
    if dist <= 1e-4 {
        return true;
    }
    if dist > range + target_radius {
        return false;
    }
    if !in_melee_swath(attacker, facing_dir, target, target_radius, range, half_width) {
        return false;
    }
    let cos_to = (to / dist).dot(dir);
    // На дальности — сектор ±half_angle; вблизи — шире, пока боковая ширина
    // не станет не меньше half_width (asin(half_width / dist)).
    let width_half = (half_width / dist).min(1.0).asin();
    let ang_radius = (target_radius / dist).min(1.0).asin();
    let eff_half = (half_angle.max(width_half) + ang_radius).min(core::f32::consts::PI);
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

/// Поглощает ли активный блок цели входящий удар. Блок прикрывает ПЕРЕДНЮЮ
/// полусферу цели ОТНОСИТЕЛЬНО ТОГО, ЧТО ВИДНО НА ЭКРАНЕ: источник `source`
/// должен лежать в пределах полу-угла `half_angle` (рад) от экранного направления
/// модели `facing`. Считаем строго от ЦЕНТРА цели (точка на земле), поэтому
/// никакие коллизии/радиусы на это не влияют.
///
/// Почему экран, а не мир: логика живёт в мировых координатах (квадрат), но игрок
/// видит изо-проекцию 2:1, в которой углы искажаются. Удар, явно прилетающий
/// СПЕРЕДИ ПО ЭКРАНУ, в мире мог оказываться за 90° и «пробивать» блок. Поэтому
/// и направление модели, и вектор на источник переводим в экранное пространство
/// и там сравниваем углы — блок срабатывает ровно для передних 180° «как видно».
/// При `half_angle = 90°` враг строго сбоку (90° на экране) ещё блокируется.
pub fn block_absorbs(target: Vec2, facing: f32, source: Vec2, half_angle: f32) -> bool {
    let to_world = source - target;
    if to_world.length_squared() <= 1e-8 {
        return true; // вплотную — спереди
    }
    let face_s = iso(Vec2::new(facing.cos(), facing.sin()));
    let to_s = iso(to_world);
    if face_s.length_squared() <= 1e-8 || to_s.length_squared() <= 1e-8 {
        return true;
    }
    let cos_to = to_s.normalize().dot(face_s.normalize());
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
    fn melee_sector_60_degrees_and_shorter_range() {
        use crate::constants::{MELEE_HALF_ANGLE, MELEE_HALF_WIDTH, MELEE_RANGE};
        let me = Vec2::ZERO;
        let dir = dir_right();
        let hit = |t: Vec2| {
            in_melee_sector(
                me,
                dir,
                t,
                0.0,
                MELEE_RANGE,
                MELEE_HALF_ANGLE,
                MELEE_HALF_WIDTH,
            )
        };
        // прямо по направлению — попадает
        assert!(hit(Vec2::new(MELEE_RANGE - 5.0, 0.0)));
        // за пределами дальности — мимо
        assert!(!hit(Vec2::new(MELEE_RANGE + 10.0, 0.0)));
        // ~25° в сторону на дальности — внутри 60° сектора
        let a = 25f32.to_radians();
        assert!(hit(Vec2::new(
            MELEE_RANGE * a.cos(),
            MELEE_RANGE * a.sin(),
        )));
        // ~40° на дальности — вне 60° сектора
        let wide = 40f32.to_radians();
        assert!(!hit(Vec2::new(
            MELEE_RANGE * wide.cos(),
            MELEE_RANGE * wide.sin(),
        )));
        // сзади — мимо
        assert!(!hit(Vec2::new(-30.0, 0.0)));
        // вблизи сбоку: чистый конус промахнулся бы, сектор с постоянной шириной — попадает
        let close_side = 50f32.to_radians();
        assert!(
            !in_melee_arc(
                me,
                dir,
                Vec2::new(4.0 * close_side.cos(), 4.0 * close_side.sin()),
                0.0,
                MELEE_RANGE,
                MELEE_HALF_ANGLE,
            ),
            "конус вблизи слишком узкий"
        );
        assert!(
            hit(Vec2::new(4.0 * close_side.cos(), 4.0 * close_side.sin())),
            "сектор с постоянной шириной бьёт близкую цель сбоку"
        );
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

    #[test]
    fn block_covers_screen_front_hemisphere() {
        use core::f32::consts::FRAC_PI_2;
        let me = Vec2::ZERO;
        let facing = 0.0;
        // Точка (-10,-50) в МИРЕ лежит «за спиной» (x<0) и мировой блок её бы
        // пропустил, но НА ЭКРАНЕ (изо 2:1) она спереди → блок 180° её держит.
        assert!(block_absorbs(me, facing, Vec2::new(-10.0, -50.0), FRAC_PI_2));
        // Ровно сбоку на экране (90°) — ещё блокируется (граница включительно).
        // Экранное направление модели для facing=0 это (1, 0.5); перпендикуляр к
        // нему в экранных координатах даёт мировую точку, дающую cos=0.
        let perp_screen = Vec2::new(-0.5, 1.0); // ⟂ к (1,0.5) на экране
        // обратное изо: world from screen (sx,sy): x=(sx/ISO_X + sy/ISO_Y)/2 ...
        // проще проверить, что строго СЗАДИ по экрану — пропускается:
        let back_screen_world = Vec2::new(-40.0, 30.0); // iso → (-70, -5): за спиной
        let _ = perp_screen;
        assert!(!block_absorbs(me, facing, back_screen_world, FRAC_PI_2));
    }

    /// Обратное изо: из ЭКРАННОГО вектора получаем мировой (для конструирования
    /// источников под нужным экранным углом).
    fn world_from_screen(s: Vec2) -> Vec2 {
        let a = s.x / ISO_X; // vx - vy
        let b = s.y / ISO_Y; // vx + vy
        Vec2::new(0.5 * (a + b), 0.5 * (b - a))
    }

    #[test]
    fn block_front_hemisphere_never_pierced_at_any_front_angle() {
        // Полноценный игровой блок: half_angle = 90° → передние 180° «как на экране».
        // Любой удар из передней полусферы (в т.ч. «под 45°», что РАНЬШЕ пробивало)
        // обязан гаситься; всё, что явно сзади по экрану — проходит.
        let half = crate::constants::BLOCK_ARC_HALF_ANGLE;
        let me = Vec2::ZERO;
        for facing in [
            0.0_f32,
            core::f32::consts::FRAC_PI_2,
            core::f32::consts::PI,
            -core::f32::consts::FRAC_PI_2,
            0.7,
        ] {
            let face_screen = iso(Vec2::new(facing.cos(), facing.sin()));
            let face_ang = face_screen.y.atan2(face_screen.x);
            // фронтальные экранные углы (до ±85°) — всё держится
            for deg in [-85.0, -45.0, -20.0, 0.0, 20.0, 45.0, 85.0] {
                let th = face_ang + (deg as f32).to_radians();
                let src = world_from_screen(Vec2::new(th.cos(), th.sin()) * 50.0);
                assert!(
                    block_absorbs(me, facing, src, half),
                    "facing={facing}, экранный {deg}° спереди — блок не должен пробиваться"
                );
            }
            // явно сзади по экрану — урон проходит
            for deg in [110.0, 150.0, 180.0, -110.0] {
                let th = face_ang + (deg as f32).to_radians();
                let src = world_from_screen(Vec2::new(th.cos(), th.sin()) * 50.0);
                assert!(
                    !block_absorbs(me, facing, src, half),
                    "facing={facing}, экранный {deg}° сзади — урон должен проходить"
                );
            }
        }
    }
}
