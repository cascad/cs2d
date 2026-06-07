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
    pub dash_speed: f32,
    pub dash_duration: f32,
    pub dash_cooldown: f32,
    pub block_establish: f32,
    pub melee_cooldown: f32,
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
            dash_speed: DASH_SPEED,
            dash_duration: DASH_DURATION,
            dash_cooldown: DASH_COOLDOWN,
            block_establish: BLOCK_ESTABLISH_TIME,
            melee_cooldown: MELEE_COOLDOWN,
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
        }
    }
}

/// Намерения игрока на этот тик.
#[derive(Clone, Copy, Debug)]
pub struct AbilityInput {
    pub move_dir: Vec2, // желаемое направление (не обязательно нормализованное)
    pub facing: f32,    // угол взгляда (для рывка, если move_dir == 0)
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
}

impl Abilities {
    /// Готов ли удар (кулдаун прошёл и хватает стамины).
    pub fn melee_ready(&self, cfg: &AbilityConfig) -> bool {
        self.melee_cd_left <= 0.0 && self.stamina >= cfg.melee_cost
    }

    /// Зафиксировать удар: ставит кулдаун и тратит стамину.
    pub fn consume_melee(&mut self, cfg: &AbilityConfig) {
        self.melee_cd_left = cfg.melee_cooldown;
        self.stamina = (self.stamina - cfg.melee_cost).max(0.0);
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

    // старт рывка (нельзя дважды одновременно)
    let mut did_dash = false;
    if inp.want_dash
        && a.dash_left <= 0.0
        && a.dash_cd_left <= 0.0
        && a.stamina >= cfg.dash_cost
    {
        let dir = if inp.move_dir.length_squared() > 1e-6 {
            inp.move_dir.normalize()
        } else {
            Vec2::new(inp.facing.cos(), inp.facing.sin())
        };
        a.dash_left = cfg.dash_duration;
        a.dash_dir = dir;
        a.dash_cd_left = cfg.dash_cooldown;
        a.stamina = (a.stamina - cfg.dash_cost).max(0.0);
        did_dash = true;
    }

    // блок: заряжается при удержании; нельзя блокировать во время рывка
    let mut blocking = false;
    if inp.want_block && a.dash_left <= 0.0 && a.stamina > 0.0 {
        a.block_charge += dt;
        if a.block_charge >= cfg.block_establish {
            blocking = true;
            a.stamina = (a.stamina - cfg.block_drain * dt).max(0.0);
        }
    } else {
        a.block_charge = 0.0;
    }

    // движение: во время рывка — dash_dir/dash_speed; иначе обычное (с замедлением при блоке)
    let (move_dir, speed) = if a.dash_left > 0.0 {
        a.dash_left = (a.dash_left - dt).max(0.0);
        (a.dash_dir, cfg.dash_speed)
    } else if blocking {
        (inp.move_dir, cfg.move_speed * cfg.block_move_mult)
    } else {
        (inp.move_dir, cfg.move_speed)
    };

    // регенерация стамины, когда не держим блок
    if !inp.want_block {
        a.stamina = (a.stamina + cfg.stamina_regen * dt).min(cfg.stamina_max);
    }

    AbilityOutput {
        move_dir,
        speed,
        blocking,
        did_dash,
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
            facing: 0.0,
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
            facing: 0.0,
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
        assert!((out.speed - DASH_SPEED).abs() < 1e-3);
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
    fn block_establishes_after_delay_and_drains() {
        let mut a = Abilities::default();
        // первый тик: блок ещё не встал (charge < establish)
        let out0 = tick_abilities(&mut a, &moving_right(false, true), 0.01, &cfg());
        assert!(!out0.blocking, "блок не встаёт мгновенно");
        // держим дольше времени установки
        let mut blocking = false;
        for _ in 0..100 {
            let out = tick_abilities(&mut a, &moving_right(false, true), 0.015, &cfg());
            blocking |= out.blocking;
        }
        assert!(blocking, "после задержки блок должен встать");
        assert!(a.stamina < STAMINA_MAX, "активный блок тратит стамину");
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
}
