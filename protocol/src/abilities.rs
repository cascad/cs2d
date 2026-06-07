//! Способности игрока: стамина, рывок (dash) и блок — единая детерминированная
//! логика для клиента (предсказание) и сервера (авторитет). За счёт общего кода
//! предсказание и авторитет считают одно и то же при одинаковых вводах.
//!
//! Melee (ближний удар) — дискретное событие, резолвится сервером (см.
//! `combat`), но его кулдаун/стоимость стамины тоже учитываются здесь.

use crate::constants::*;
use glam::Vec2;

/// Настраиваемые параметры способностей. По умолчанию берутся из `constants`.
#[derive(Clone, Copy, Debug)]
pub struct AbilityConfig {
    pub stamina_max: f32,
    pub stamina_regen: f32,
    pub block_drain: f32,
    pub dash_cost: f32,
    pub melee_cost: f32,
    pub move_speed: f32,
    pub block_move_mult: f32,
    pub fwd_mult: f32,
    pub side_mult: f32,
    pub back_mult: f32,
    pub fwd_cos: f32,
    pub back_cos: f32,
    pub dash_speed: f32,
    pub dash_duration: f32,
    pub dash_cooldown: f32,
    pub block_establish: f32,
    pub melee_cooldown: f32,
    pub turn_rate: f32,         // скорость разворота к прицелу, рад/сек
    pub attack_facing_lock: f32, // заморозка разворота на время удара, сек
}

impl Default for AbilityConfig {
    fn default() -> Self {
        Self {
            stamina_max: STAMINA_MAX,
            stamina_regen: STAMINA_REGEN_PER_SEC,
            block_drain: BLOCK_STAMINA_DRAIN_PER_SEC,
            dash_cost: DASH_STAMINA_COST,
            melee_cost: MELEE_STAMINA_COST,
            move_speed: MOVE_SPEED,
            block_move_mult: BLOCK_MOVE_MULT,
            fwd_mult: MOVE_FWD_MULT,
            side_mult: MOVE_SIDE_MULT,
            back_mult: MOVE_BACK_MULT,
            fwd_cos: MOVE_FWD_COS,
            back_cos: MOVE_BACK_COS,
            dash_speed: DASH_SPEED,
            dash_duration: DASH_DURATION,
            dash_cooldown: DASH_COOLDOWN,
            block_establish: BLOCK_ESTABLISH_TIME,
            melee_cooldown: MELEE_COOLDOWN,
            turn_rate: TURN_RATE,
            attack_facing_lock: ATTACK_FACING_LOCK,
        }
    }
}

/// Изменяемое состояние способностей одного игрока.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub stamina: f32,
    pub dash_left: f32,   // сек до конца текущего рывка
    pub dash_dir: Vec2,   // направление рывка
    pub dash_cd_left: f32, // сек до готовности рывка
    pub melee_cd_left: f32, // сек до готовности удара
    pub block_charge: f32, // сколько непрерывно держится блок
    pub facing: f32,          // ТЕКУЩИЙ угол модели (плавно догоняет прицел)
    pub attack_lock_left: f32, // сек заморозки разворота из-за удара
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            stamina: STAMINA_MAX,
            dash_left: 0.0,
            dash_dir: Vec2::ZERO,
            dash_cd_left: 0.0,
            melee_cd_left: 0.0,
            block_charge: 0.0,
            facing: 0.0,
            attack_lock_left: 0.0,
        }
    }
}

/// Намерения игрока на этот тик.
#[derive(Clone, Copy, Debug)]
pub struct AbilityInput {
    pub move_dir: Vec2, // желаемое направление (не обязательно нормализованное)
    pub aim: f32,       // ЖЕЛАЕМЫЙ угол (курсор) — модель плавно к нему разворачивается
    pub want_dash: bool,
    pub want_block: bool,
}

/// Результат тика: эффективное движение и состояние блока.
#[derive(Clone, Copy, Debug)]
pub struct AbilityOutput {
    pub move_dir: Vec2, // эффективное направление движения этого тика
    pub speed: f32,     // эффективная скорость
    pub blocking: bool, // блок установлен и активен
    pub did_dash: bool, // в этот тик стартовал рывок
    pub facing: f32,    // ТЕКУЩИЙ угол модели после разворота этого тика
}

impl Abilities {
    /// Готов ли удар (кулдаун прошёл и хватает стамины).
    pub fn melee_ready(&self, cfg: &AbilityConfig) -> bool {
        self.melee_cd_left <= 0.0 && self.stamina >= cfg.melee_cost
    }

    /// Зафиксировать удар: кулдаун, стамина и заморозка разворота на время удара.
    pub fn consume_melee(&mut self, cfg: &AbilityConfig) {
        self.melee_cd_left = cfg.melee_cooldown;
        self.stamina = (self.stamina - cfg.melee_cost).max(0.0);
        self.attack_lock_left = cfg.attack_facing_lock;
    }
}

/// Нормализация угла в (−π, π].
#[inline]
fn wrap_pi(a: f32) -> f32 {
    use core::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

/// Повернуть `cur` к `target` не более чем на `max_step` (по короткой дуге).
#[inline]
fn turn_toward(cur: f32, target: f32, max_step: f32) -> f32 {
    let diff = wrap_pi(target - cur);
    if diff.abs() <= max_step {
        target
    } else {
        wrap_pi(cur + max_step * diff.signum())
    }
}

/// Множитель скорости, который выравнивает ЭКРАННУЮ скорость во все стороны при
/// изометрии 2:1.
///
/// Проблема: логика движения живёт в МИРОВЫХ координатах (квадрат), а на экран
/// мир проецируется 2:1 (`world_to_screen`: Y сплющен вдвое). Из-за этого одна и
/// та же мировая скорость даёт РАЗНУЮ скорость по экрану — «влево-вправо»
/// (экранная горизонталь, мир-направление (1,−1)) в ДВА раза быстрее, чем
/// «вверх-вниз» (экранная вертикаль, мир-направление (1,1)). Игрок видит экран,
/// поэтому ожидает одинаковую скорость во все стороны.
///
/// Решение: домножаем мировую скорость на 1/|world_to_screen(dir)|. Тогда
/// экранная скорость = заданной (одинакова по всем направлениям), а мировая
/// скорость по направлениям меняется — это и компенсирует проекцию. Считается
/// детерминированно, одинаково на клиенте и сервере (общий код). Возвращает 1.0
/// при нулевом направлении.
#[inline]
pub fn screen_speed_norm(world_dir: Vec2) -> f32 {
    let l2 = world_dir.length_squared();
    if l2 < 1e-6 {
        return 1.0;
    }
    let d = world_dir / l2.sqrt();
    // экранная проекция единичного мирового направления (как `world_to_screen`)
    let sx = (d.x - d.y) * ISO_X;
    let sy = (d.x + d.y) * ISO_Y;
    let len = (sx * sx + sy * sy).sqrt();
    if len > 1e-6 {
        1.0 / len
    } else {
        1.0
    }
}

/// Один тик способностей: кулдауны, старт/продолжение рывка, установка блока,
/// расход и регенерация стамины. Возвращает эффективные параметры движения.
pub fn tick_abilities(
    a: &mut Abilities,
    inp: &AbilityInput,
    dt: f32,
    cfg: &AbilityConfig,
) -> AbilityOutput {
    // кулдауны
    a.dash_cd_left = (a.dash_cd_left - dt).max(0.0);
    a.melee_cd_left = (a.melee_cd_left - dt).max(0.0);
    a.attack_lock_left = (a.attack_lock_left - dt).max(0.0);

    // старт рывка (нельзя дважды одновременно); направление берём из текущего
    // (ещё не довёрнутого) угла, если нет ввода движения
    let mut did_dash = false;
    if inp.want_dash
        && a.dash_left <= 0.0
        && a.dash_cd_left <= 0.0
        && a.attack_lock_left <= 0.0 // нельзя дэшиться, пока не доиграл удар
        && a.stamina >= cfg.dash_cost
    {
        let dir = if inp.move_dir.length_squared() > 1e-6 {
            inp.move_dir.normalize()
        } else {
            Vec2::new(a.facing.cos(), a.facing.sin())
        };
        a.dash_left = cfg.dash_duration;
        a.dash_dir = dir;
        a.dash_cd_left = cfg.dash_cooldown;
        a.stamina = (a.stamina - cfg.dash_cost).max(0.0);
        did_dash = true;
    }

    // Плавный разворот модели к прицелу. Во время рывка (вкл. только что
    // начатого) или удара разворот ЗАМОРОЖЕН — одинаково на клиенте и сервере.
    let facing_locked = a.dash_left > 0.0 || a.attack_lock_left > 0.0;
    if !facing_locked {
        a.facing = turn_toward(a.facing, inp.aim, cfg.turn_rate * dt);
    }

    // блок: держится ВСЁ время, пока зажата mouse2 (после короткого замаха);
    // нельзя блокировать во время рывка/удара. Стамину не тратит — не пропадает.
    let mut blocking = false;
    if inp.want_block && a.dash_left <= 0.0 && a.attack_lock_left <= 0.0 {
        a.block_charge += dt;
        if a.block_charge >= cfg.block_establish {
            blocking = true;
        }
    } else {
        a.block_charge = 0.0;
    }

    // движение:
    // - во время рывка — только dash_dir/dash_speed (обычного управления нет);
    // - во время удара (attack_lock) — стоим на месте (ни шага, ни разворота);
    // - иначе обычное (замедление при блоке + анизотропия ОТНОСИТЕЛЬНО ПРИЦЕЛА).
    let (move_dir, speed) = if a.dash_left > 0.0 {
        a.dash_left = (a.dash_left - dt).max(0.0);
        (a.dash_dir, cfg.dash_speed)
    } else if a.attack_lock_left > 0.0 {
        (Vec2::ZERO, 0.0)
    } else if blocking {
        // во время блока — шаг в ЛЮБУЮ сторону (медленно), без анизотропии
        (inp.move_dir, cfg.move_speed * cfg.block_move_mult)
    } else {
        // обычная скорость ОДИНАКОВА во все стороны (вперёд/боком/спиной) — без
        // анизотропии по прицелу; замедляет только блок
        (inp.move_dir, cfg.move_speed)
    };

    // ВЫРАВНИВАНИЕ ЭКРАННОЙ СКОРОСТИ: домножаем мировую скорость так, чтобы по
    // экрану движение во все стороны шло одинаково быстро (иначе при изо 2:1
    // «влево-вправо» вдвое быстрее «вверх-вниз»). Распространяется на ходьбу,
    // блок и рывок. Одинаково на клиенте и сервере → без рассинхрона.
    let speed = speed * screen_speed_norm(move_dir);

    // регенерация стамины (блок больше не тратит стамину, копим всегда)
    a.stamina = (a.stamina + cfg.stamina_regen * dt).min(cfg.stamina_max);

    AbilityOutput {
        move_dir,
        speed,
        blocking,
        did_dash,
        facing: a.facing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AbilityConfig {
        AbilityConfig::default()
    }

    fn moving_right(dash: bool, block: bool) -> AbilityInput {
        AbilityInput {
            move_dir: Vec2::new(1.0, 0.0),
            aim: 0.0,
            want_dash: dash,
            want_block: block,
        }
    }

    #[test]
    fn idle_regenerates_stamina_up_to_max() {
        let mut a = Abilities {
            stamina: 10.0,
            ..Default::default()
        };
        let inp = AbilityInput {
            move_dir: Vec2::ZERO,
            aim: 0.0,
            want_dash: false,
            want_block: false,
        };
        for _ in 0..1000 {
            tick_abilities(&mut a, &inp, 0.015, &cfg());
        }
        assert!((a.stamina - STAMINA_MAX).abs() < 1e-3);
    }

    #[test]
    fn dash_triggers_speed_and_consumes_stamina_and_cooldown() {
        let mut a = Abilities::default();
        let out = tick_abilities(&mut a, &moving_right(true, false), 0.015, &cfg());
        assert!(out.did_dash);
        // скорость рывка включает выравнивание экранной скорости для (1,0)
        let want = DASH_SPEED * screen_speed_norm(Vec2::new(1.0, 0.0));
        assert!((out.speed - want).abs() < 1e-3);
        assert!(a.stamina < STAMINA_MAX); // потратили
        assert!(a.dash_cd_left > 0.0); // ушёл на кулдаун
    }

    #[test]
    fn dash_blocked_during_cooldown() {
        let mut a = Abilities::default();
        tick_abilities(&mut a, &moving_right(true, false), 0.015, &cfg());
        // сразу повторный рывок невозможен (кулдаун)
        // дождёмся конца самого рывка, но кулдаун ещё идёт
        for _ in 0..20 {
            let out = tick_abilities(&mut a, &moving_right(true, false), 0.015, &cfg());
            assert!(!out.did_dash, "повторный рывок не должен стартовать на кулдауне");
        }
    }

    #[test]
    fn dash_needs_stamina() {
        let mut a = Abilities {
            stamina: 0.0,
            ..Default::default()
        };
        let out = tick_abilities(&mut a, &moving_right(true, false), 0.015, &cfg());
        assert!(!out.did_dash, "без стамины рывок невозможен");
    }

    #[test]
    fn block_establishes_after_delay_and_holds() {
        let mut a = Abilities::default();
        // первый тик: блок ещё не встал (charge < establish)
        let out0 = tick_abilities(&mut a, &moving_right(false, true), 0.01, &cfg());
        assert!(!out0.blocking, "блок не встаёт мгновенно");
        // держим — после задержки встаёт и НЕ пропадает (стамину не тратит)
        let mut out = out0;
        for _ in 0..200 {
            out = tick_abilities(&mut a, &moving_right(false, true), 0.015, &cfg());
        }
        assert!(out.blocking, "блок держится всё время, пока зажат");
        // скорость во время блока = медленный шаг (move_speed * block_move_mult) с
        // выравниванием экранной скорости для направления (1,0)
        let want = MOVE_SPEED * BLOCK_MOVE_MULT * screen_speed_norm(Vec2::new(1.0, 0.0));
        assert!((out.speed - want).abs() < 1e-3, "блок = шаг: {}", out.speed);
    }

    #[test]
    fn releasing_block_resets_charge() {
        let mut a = Abilities::default();
        tick_abilities(&mut a, &moving_right(false, true), 0.1, &cfg());
        tick_abilities(&mut a, &moving_right(false, false), 0.015, &cfg());
        assert_eq!(a.block_charge, 0.0);
    }

    #[test]
    fn melee_ready_and_consume() {
        let mut a = Abilities::default();
        assert!(a.melee_ready(&cfg()));
        a.consume_melee(&cfg());
        assert!(!a.melee_ready(&cfg()));
        assert!(a.melee_cd_left > 0.0);
    }

    fn aim_at(aim: f32) -> AbilityInput {
        AbilityInput {
            move_dir: Vec2::ZERO,
            aim,
            want_dash: false,
            want_block: false,
        }
    }

    #[test]
    fn facing_turns_gradually_then_reaches_aim() {
        use core::f32::consts::FRAC_PI_2;
        let mut a = Abilities::default(); // facing == 0
        let out1 = tick_abilities(&mut a, &aim_at(FRAC_PI_2), 0.016, &cfg());
        assert!(
            out1.facing > 0.0 && out1.facing < FRAC_PI_2,
            "первый тик — лишь частичный поворот, не мгновенный"
        );
        for _ in 0..200 {
            tick_abilities(&mut a, &aim_at(FRAC_PI_2), 0.016, &cfg());
        }
        assert!(
            (a.facing - FRAC_PI_2).abs() < 1e-3,
            "со временем доворачивается ровно до прицела"
        );
    }

    #[test]
    fn speed_is_constant_regardless_of_aim_direction() {
        // вне блока скорость одинакова во все стороны относительно прицела
        // (анизотропия вперёд/боком/спиной убрана). Замедляет только блок.
        let mut a = Abilities::default();
        let fwd = tick_abilities(&mut a, &moving_right(false, false), 0.016, &cfg());
        let mut b = Abilities { facing: core::f32::consts::PI, ..Default::default() };
        let back = tick_abilities(
            &mut b,
            &AbilityInput {
                move_dir: Vec2::new(1.0, 0.0),
                aim: core::f32::consts::PI,
                want_dash: false,
                want_block: false,
            },
            0.016,
            &cfg(),
        );
        assert!(
            (fwd.speed - back.speed).abs() < 1e-3,
            "скорость не зависит от направления взгляда"
        );
        let want = MOVE_SPEED * screen_speed_norm(Vec2::new(1.0, 0.0));
        assert!((fwd.speed - want).abs() < 1e-3, "обычная скорость = move_speed");
    }

    #[test]
    fn screen_speed_is_uniform_across_directions() {
        // мировая скорость, домноженная на screen_speed_norm, даёт ОДИНАКОВУЮ
        // экранную скорость во все стороны (фикс изо 2:1: горизонталь была вдвое
        // быстрее вертикали).
        let dirs = [
            Vec2::new(1.0, 1.0),   // экран: вверх
            Vec2::new(-1.0, -1.0), // экран: вниз
            Vec2::new(1.0, -1.0),  // экран: вправо
            Vec2::new(-1.0, 1.0),  // экран: влево
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
        ];
        let mut screen_speeds = Vec::new();
        for d in dirs {
            let dir = d.normalize();
            let world_speed = MOVE_SPEED * screen_speed_norm(dir);
            // экранная скорость = |world_to_screen(dir * world_speed)|
            let v = dir * world_speed;
            let sx = (v.x - v.y) * ISO_X;
            let sy = (v.x + v.y) * ISO_Y;
            screen_speeds.push((sx * sx + sy * sy).sqrt());
        }
        let first = screen_speeds[0];
        for s in &screen_speeds {
            assert!((s - first).abs() < 1e-2, "экранные скорости должны совпадать: {screen_speeds:?}");
        }
        // и равны заданной (MOVE_SPEED)
        assert!((first - MOVE_SPEED).abs() < 1e-2);
    }

    #[test]
    fn attack_lock_stops_movement() {
        let mut a = Abilities::default();
        a.consume_melee(&cfg());
        let out = tick_abilities(&mut a, &moving_right(false, false), 0.016, &cfg());
        assert_eq!(out.speed, 0.0, "во время удара игрок стоит");
        assert_eq!(out.move_dir, Vec2::ZERO);
    }

    #[test]
    fn melee_and_dash_freeze_facing() {
        use core::f32::consts::PI;
        // удар морозит разворот
        let mut a = Abilities::default();
        a.consume_melee(&cfg());
        let out = tick_abilities(&mut a, &aim_at(PI), 0.016, &cfg());
        assert_eq!(out.facing, 0.0, "во время удара модель не разворачивается");

        // рывок тоже морозит разворот
        let mut b = Abilities::default();
        let inp = AbilityInput {
            move_dir: Vec2::new(1.0, 0.0),
            aim: PI,
            want_dash: true,
            want_block: false,
        };
        let out = tick_abilities(&mut b, &inp, 0.016, &cfg());
        assert!(out.did_dash);
        assert_eq!(out.facing, 0.0, "во время рывка модель не разворачивается");
    }
}
