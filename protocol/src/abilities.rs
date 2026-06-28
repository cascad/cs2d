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
    /// Минимум стамины, чтобы держать щит поднятым. Не хватает — щит опускается
    /// (и удар проходит). Совпадает со стоимостью поглощения одного удара.
    pub block_min_stamina: f32,
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
    /// Допуск готовности удара (сек). На КЛИЕНТЕ = 0 (строго по своему КД), на
    /// СЕРВЕРЕ = `MELEE_CD_GRACE` — гасит ±1 тик рассинхрона двух таймеров, чтобы
    /// предсказанный честным клиентом взмах гарантированно наносил урон.
    pub melee_grace: f32,
    /// Допуск готовности рывка (как `melee_grace`): на клиенте 0, на сервере
    /// `DASH_CD_GRACE`. Гарантирует, что предсказанный честным клиентом рывок
    /// выполнится и на сервере — иначе сервер двигал бы игрока без анимации.
    pub dash_grace: f32,
    pub turn_rate: f32,         // скорость разворота к прицелу, рад/сек
    pub attack_facing_lock: f32, // заморозка разворота на время удара, сек
    // --- Stun (удар щитом, Q) ---
    pub stun_cooldown: f32,   // КД способности
    pub stun_cost: f32,       // расход стамины на удар щитом
    pub stun_duration: f32,   // длительность оглушения цели
    pub stun_facing_lock: f32, // заморозка движения/разворота на время Kick
    /// Допуск готовности удара щитом (как `melee_grace`): на клиенте 0, на сервере
    /// `STUN_CD_GRACE`.
    pub stun_grace: f32,
}

impl Default for AbilityConfig {
    fn default() -> Self {
        Self {
            stamina_max: STAMINA_MAX,
            stamina_regen: STAMINA_REGEN_PER_SEC,
            block_drain: BLOCK_STAMINA_DRAIN_PER_SEC,
            block_min_stamina: BLOCK_HIT_STAMINA_COST,
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
            // по умолчанию строго (клиент); сервер поднимает до MELEE_CD_GRACE
            melee_grace: 0.0,
            // по умолчанию строго (клиент); сервер поднимает до DASH_CD_GRACE
            dash_grace: 0.0,
            turn_rate: TURN_RATE,
            attack_facing_lock: ATTACK_FACING_LOCK,
            stun_cooldown: STUN_COOLDOWN,
            stun_cost: STUN_STAMINA_COST,
            stun_duration: STUN_DURATION,
            stun_facing_lock: STUN_FACING_LOCK,
            // по умолчанию строго (клиент); сервер поднимает до STUN_CD_GRACE
            stun_grace: 0.0,
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
    pub stun_cd_left: f32, // сек до готовности удара щитом (Q)
    /// Сек оглушения, оставшегося НА САМОМ игроке: пока >0, управление
    /// игнорируется (нельзя ходить/бить/блокировать/разворачиваться). Авторитет —
    /// сервер; клиент сидит это значение из снапшота для совпадения предсказания.
    pub stun_left: f32,
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
            stun_cd_left: 0.0,
            stun_left: 0.0,
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
    pub want_attack: bool, // запрос ближнего удара на этом тике
    pub want_stun: bool,   // запрос удара щитом (Q) на этом тике
}

/// Результат тика: эффективное движение и состояние блока.
#[derive(Clone, Copy, Debug)]
pub struct AbilityOutput {
    pub move_dir: Vec2,    // эффективное направление движения этого тика
    pub speed: f32,        // эффективная скорость
    pub blocking: bool,    // блок установлен и активен
    pub did_dash: bool,    // в этот тик стартовал рывок
    pub dash_dir: Vec2,    // направление рывка (для анимации переката) — валидно при did_dash
    pub did_attack: bool,  // в этот тик стартовал ближний удар
    pub attack_dir: Vec2,  // направление удара (по прицелу) — для FX и резолва урона
    pub did_stun: bool,    // в этот тик стартовал удар щитом (Q)
    pub stun_dir: Vec2,    // направление удара щитом (по прицелу) — для FX и резолва
    pub stunned: bool,     // игрок СЕЙЧАС оглушён (управление игнорируется)
    pub facing: f32,       // ТЕКУЩИЙ угол модели после разворота этого тика
}

impl Abilities {
    /// Готов ли удар (кулдаун прошёл — с учётом допуска `melee_grace` — и хватает
    /// стамины). На клиенте `melee_grace == 0` (строго), на сервере он чуть больше,
    /// чтобы погасить ±1 тик рассинхрона двух таймеров.
    pub fn melee_ready(&self, cfg: &AbilityConfig) -> bool {
        self.melee_cd_left <= cfg.melee_grace && self.stamina >= cfg.melee_cost
    }

    /// Зафиксировать удар: кулдаун, стамина и заморозка разворота на время удара.
    pub fn consume_melee(&mut self, cfg: &AbilityConfig) {
        self.melee_cd_left = cfg.melee_cooldown;
        self.stamina = (self.stamina - cfg.melee_cost).max(0.0);
        self.attack_lock_left = cfg.attack_facing_lock;
    }

    /// Готов ли удар щитом (КД с допуском `stun_grace` прошёл и хватает стамины).
    pub fn stun_ready(&self, cfg: &AbilityConfig) -> bool {
        self.stun_cd_left <= cfg.stun_grace && self.stamina >= cfg.stun_cost
    }

    /// Зафиксировать удар щитом: КД, стамина и заморозка движения/разворота на
    /// время анимации Kick.
    pub fn consume_stun(&mut self, cfg: &AbilityConfig) {
        self.stun_cd_left = cfg.stun_cooldown;
        self.stamina = (self.stamina - cfg.stun_cost).max(0.0);
        self.attack_lock_left = cfg.stun_facing_lock;
    }

    /// Наложить/обновить оглушение на ЭТОГО игрока (сбрасывает таймер — «новый стан
    /// обновляет полоску»). Прерывает текущие действия (рывок/удар/блок-заряд).
    pub fn apply_stun(&mut self, cfg: &AbilityConfig) {
        self.stun_left = cfg.stun_duration;
        self.dash_left = 0.0;
        self.attack_lock_left = 0.0;
        self.block_charge = 0.0;
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

/// Один тик способностей: кулдауны, старт/продолжение рывка, установка блока,
/// расход и регенерация стамины. Возвращает эффективные параметры движения.
pub fn tick_abilities(
    a: &mut Abilities,
    inp: &AbilityInput,
    dt: f32,
    cfg: &AbilityConfig,
) -> AbilityOutput {
    // кулдауны (тикают всегда, даже под станом — чтобы по выходу из стана КД были
    // в актуальном состоянии, а стамина восстанавливалась)
    a.dash_cd_left = (a.dash_cd_left - dt).max(0.0);
    a.melee_cd_left = (a.melee_cd_left - dt).max(0.0);
    a.stun_cd_left = (a.stun_cd_left - dt).max(0.0);
    a.attack_lock_left = (a.attack_lock_left - dt).max(0.0);

    // ОГЛУШЕНИЕ: пока тикает stun_left — игрок полностью неуправляем. Гасим все
    // намерения (ни шага, ни удара, ни блока, ни разворота). Это считается
    // одинаково на клиенте (предсказание) и сервере (авторитет): клиент сидит
    // stun_left из снапшота, поэтому замирает синхронно с сервером.
    if a.stun_left > 0.0 {
        a.stun_left = (a.stun_left - dt).max(0.0);
        a.dash_left = 0.0;
        a.block_charge = 0.0;
        // стамина продолжает восстанавливаться, пока стоим оглушённые
        a.stamina = (a.stamina + cfg.stamina_regen * dt).min(cfg.stamina_max);
        return AbilityOutput {
            move_dir: Vec2::ZERO,
            speed: 0.0,
            blocking: false,
            did_dash: false,
            dash_dir: a.dash_dir,
            did_attack: false,
            attack_dir: Vec2::ZERO,
            did_stun: false,
            stun_dir: Vec2::ZERO,
            stunned: true,
            facing: a.facing,
        };
    }

    // старт рывка (нельзя дважды одновременно); направление берём из текущего
    // (ещё не довёрнутого) угла, если нет ввода движения. Рывок приоритетнее
    // удара: при удержании ЛКМ игрок всё равно может дэшнуться (иначе бы вечно
    // «висел» в атаке и не мог уйти).
    let mut did_dash = false;
    if inp.want_dash
        && a.dash_left <= 0.0
        && a.dash_cd_left <= cfg.dash_grace
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

    // Ближний удар (melee) — ДИСКРЕТНОЕ событие в общем тике, симметрично рывку.
    // Готовность (КД + стамина) проверяется и «фиксируется» В ТОМ ЖЕ тике, где
    // тикается КД, поэтому клиент-предсказание и сервер-авторитет дают идентичный
    // `did_attack` при одинаковом вводе — нет гонки «проверил до тика, тикнул
    // после», из-за которой второй удар впритык к концу КД терялся. Во время
    // рывка (вкл. только что начатого) бить нельзя. consume_melee ставит
    // КД/стамину/заморозку разворота.
    let mut did_attack = false;
    let mut attack_dir = Vec2::ZERO;
    if inp.want_attack && a.dash_left <= 0.0 && a.melee_ready(cfg) {
        a.consume_melee(cfg);
        did_attack = true;
        attack_dir = Vec2::new(inp.aim.cos(), inp.aim.sin());
    }

    // Удар щитом (stun, Q) — ещё одно дискретное событие. Взаимоисключим с melee
    // в этот тик (нельзя одновременно махать мечом и бить щитом); во время рывка
    // или уже идущего удара — нельзя. consume_stun ставит КД/стамину/заморозку.
    let mut did_stun = false;
    let mut stun_dir = Vec2::ZERO;
    if inp.want_stun
        && !did_attack
        && a.dash_left <= 0.0
        && a.attack_lock_left <= 0.0
        && a.stun_ready(cfg)
    {
        a.consume_stun(cfg);
        did_stun = true;
        stun_dir = Vec2::new(inp.aim.cos(), inp.aim.sin());
    }

    // Плавный разворот модели к прицелу. Во время рывка (вкл. только что
    // начатого) или удара разворот ЗАМОРОЖЕН — одинаково на клиенте и сервере.
    let facing_locked = a.dash_left > 0.0 || a.attack_lock_left > 0.0;
    if !facing_locked {
        a.facing = turn_toward(a.facing, inp.aim, cfg.turn_rate * dt);
    }

    // блок: держится, пока зажата mouse2 (после короткого замаха) И ПОКА ХВАТАЕТ
    // стамины. Кулдауна нет; удержание само по себе стамину НЕ тратит (списывается
    // только при попадании по блоку — на сервере). Если стамины не хватает на
    // удар (< block_min_stamina) — щит ОПУСКАЕТСЯ, и удар проходит; как только
    // стамина восстановится выше порога, щит поднимается снова (мгновенно, замах
    // уже накоплен). Заряд копим независимо от стамины, чтобы не было повторного
    // «замаха» каждый раз при просадке.
    let mut blocking = false;
    if inp.want_block && a.dash_left <= 0.0 && a.attack_lock_left <= 0.0 {
        a.block_charge += dt;
        if a.block_charge >= cfg.block_establish && a.stamina >= cfg.block_min_stamina {
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

    // Скорость МОНОТОННА: одно и то же значение (`move_speed`) во все стороны в
    // МИРОВЫХ координатах. Никакой анизотропии/выравнивания под экран — движение
    // одинаковое по геймплею в любом направлении (детерминированно на клиенте и
    // сервере). Изо-проекция остаётся чисто визуальной.

    // стамина: удержание блока ничего не стоит (расход только при попадании по
    // блоку — считается на сервере), поэтому просто восстанавливаем со временем.
    a.stamina = (a.stamina + cfg.stamina_regen * dt).min(cfg.stamina_max);

    AbilityOutput {
        move_dir,
        speed,
        blocking,
        did_dash,
        dash_dir: a.dash_dir,
        did_attack,
        attack_dir,
        did_stun,
        stun_dir,
        stunned: false,
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
            want_attack: false,
            want_stun: false,
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
            want_attack: false,
            want_stun: false,
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
        // скорость рывка одинакова во все стороны (без выравнивания под экран)
        let want = DASH_SPEED;
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
        // скорость во время блока = медленный шаг (move_speed * block_move_mult),
        // одинаково во все стороны (без выравнивания под экран)
        let want = MOVE_SPEED * BLOCK_MOVE_MULT;
        assert!((out.speed - want).abs() < 1e-3, "блок = шаг: {}", out.speed);
    }

    #[test]
    fn holding_block_does_not_drain_stamina_and_never_drops() {
        // Блок не имеет кулдауна и не тратит стамину сам по себе: пока кнопка
        // зажата, он держится всё время и не «дёргается». Стамина даже растёт.
        let mut a = Abilities {
            stamina: 50.0,
            ..Default::default()
        };
        // даём блоку установиться (короткий замах block_establish)
        for _ in 0..20 {
            tick_abilities(&mut a, &moving_right(false, true), 0.015, &cfg());
        }
        for _ in 0..4000 {
            let out = tick_abilities(&mut a, &moving_right(false, true), 0.015, &cfg());
            assert!(out.blocking, "блок не должен спадать, пока зажата кнопка");
        }
        assert!(
            (a.stamina - STAMINA_MAX).abs() < 1e-3,
            "удержание блока не тратит стамину (она восстанавливается)"
        );
    }

    #[test]
    fn block_drops_when_out_of_stamina_then_returns_after_regen() {
        // Стамины нет: щит НЕ поднимается, даже если кнопка зажата (урон пройдёт).
        // Когда стамина восстановится выше порога — щит снова встаёт (замах уже
        // накоплен, без повторной задержки).
        let cfg = cfg();
        let mut a = Abilities {
            stamina: 0.0,
            ..Default::default()
        };
        // ~0.15с удержания: замах накопился (>=block_establish), но стамина < порога
        let mut out = tick_abilities(&mut a, &moving_right(false, true), 0.01, &cfg);
        for _ in 0..14 {
            out = tick_abilities(&mut a, &moving_right(false, true), 0.01, &cfg);
        }
        assert!(
            a.stamina < cfg.block_min_stamina,
            "стамина ещё ниже порога: {}",
            a.stamina
        );
        assert!(!out.blocking, "без стамины щит опущен, урон проходит");
        // держим дальше — стамина регенерится выше порога, щит поднимается
        for _ in 0..200 {
            out = tick_abilities(&mut a, &moving_right(false, true), 0.01, &cfg);
        }
        assert!(a.stamina >= cfg.block_min_stamina);
        assert!(out.blocking, "после восстановления стамины щит снова держит");
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

    fn attack_at(aim: f32) -> AbilityInput {
        AbilityInput {
            move_dir: Vec2::ZERO,
            aim,
            want_dash: false,
            want_block: false,
            want_attack: true,
            want_stun: false,
        }
    }

    #[test]
    fn melee_fires_through_tick_and_respects_cooldown() {
        let cfg = cfg();
        let mut a = Abilities::default();

        // первый удар проходит сразу
        let out = tick_abilities(&mut a, &attack_at(0.0), TICK_DT, &cfg);
        assert!(out.did_attack, "первый удар должен пройти");
        assert!((out.attack_dir - Vec2::new(1.0, 0.0)).length() < 1e-5);

        // удержание ЛКМ: почти весь КД повторного удара быть не должно
        let need = (cfg.melee_cooldown / TICK_DT).ceil() as i32;
        for _ in 0..(need - 2) {
            let o = tick_abilities(&mut a, &attack_at(0.0), TICK_DT, &cfg);
            assert!(!o.did_attack, "во время КД удар не повторяется");
        }
        // в пределах нескольких ближайших тиков (граница ±1 тик из-за остатка f32)
        // второй удар обязан пройти ровно один раз
        let mut fired = 0;
        for _ in 0..4 {
            if tick_abilities(&mut a, &attack_at(0.0), TICK_DT, &cfg).did_attack {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "после КД удар снова доступен ровно один раз");
    }

    #[test]
    fn melee_blocked_during_dash() {
        let cfg = cfg();
        let mut a = Abilities::default();
        // стартуем рывок и одновременно просим удар — удар не должен пройти
        let inp = AbilityInput {
            move_dir: Vec2::new(1.0, 0.0),
            aim: 0.0,
            want_dash: true,
            want_block: false,
            want_attack: true,
            want_stun: false,
        };
        let out = tick_abilities(&mut a, &inp, TICK_DT, &cfg);
        assert!(out.did_dash);
        assert!(!out.did_attack, "во время рывка бить нельзя");
    }

    fn aim_at(aim: f32) -> AbilityInput {
        AbilityInput {
            move_dir: Vec2::ZERO,
            aim,
            want_dash: false,
            want_block: false,
            want_attack: false,
            want_stun: false,
        }
    }

    fn stun_at(aim: f32) -> AbilityInput {
        AbilityInput {
            move_dir: Vec2::ZERO,
            aim,
            want_dash: false,
            want_block: false,
            want_attack: false,
            want_stun: true,
        }
    }

    #[test]
    fn stun_bash_triggers_and_consumes_cd_and_stamina() {
        let cfg = cfg();
        let mut a = Abilities::default();
        let out = tick_abilities(&mut a, &stun_at(0.0), TICK_DT, &cfg);
        assert!(out.did_stun, "удар щитом стартовал");
        assert!((out.stun_dir - Vec2::new(1.0, 0.0)).length() < 1e-4);
        assert!(a.stun_cd_left > 0.0, "встал КД щита");
        // стамина списана на удар (в тот же тик идёт небольшая регенерация, поэтому
        // допускаем зазор в один шаг реген.)
        let spent = STAMINA_MAX - a.stamina;
        assert!(
            spent > STUN_STAMINA_COST - 1.0,
            "стамина списана на удар щитом: потрачено {spent}"
        );
        // повторный удар сразу нельзя — КД не прошёл (клиентский grace = 0)
        let out2 = tick_abilities(&mut a, &stun_at(0.0), TICK_DT, &cfg);
        assert!(!out2.did_stun, "пока КД щита не прошёл — повторно нельзя");
    }

    #[test]
    fn stun_and_melee_are_mutually_exclusive_in_one_tick() {
        let cfg = cfg();
        let mut a = Abilities::default();
        let inp = AbilityInput {
            move_dir: Vec2::ZERO,
            aim: 0.0,
            want_dash: false,
            want_block: false,
            want_attack: true,
            want_stun: true,
        };
        let out = tick_abilities(&mut a, &inp, TICK_DT, &cfg);
        assert!(out.did_attack, "melee имеет приоритет в этот тик");
        assert!(!out.did_stun, "одновременно щит не бьёт");
    }

    #[test]
    fn stunned_player_ignores_all_input_and_recovers() {
        let cfg = cfg();
        let mut a = Abilities::default();
        a.apply_stun(&cfg); // оглушены на STUN_DURATION
        assert!(a.stun_left > 0.0);
        // пытаемся ходить/бить/дэшиться/блокировать — всё игнорируется
        let inp = AbilityInput {
            move_dir: Vec2::new(1.0, 0.0),
            aim: core::f32::consts::PI,
            want_dash: true,
            want_block: true,
            want_attack: true,
            want_stun: true,
        };
        let out = tick_abilities(&mut a, &inp, TICK_DT, &cfg);
        assert!(out.stunned, "помечен как оглушённый");
        assert_eq!(out.move_dir, Vec2::ZERO, "не двигается");
        assert_eq!(out.speed, 0.0);
        assert!(!out.did_dash && !out.did_attack && !out.did_stun, "никаких действий");
        assert_eq!(out.facing, 0.0, "не разворачивается");
        assert!(!out.blocking, "блок под станом не ставится");

        // по истечении времени снова управляем
        let steps = (STUN_DURATION / TICK_DT) as usize + 2;
        for _ in 0..steps {
            tick_abilities(&mut a, &aim_at(0.0), TICK_DT, &cfg);
        }
        assert_eq!(a.stun_left, 0.0, "стан закончился");
        let out2 = tick_abilities(&mut a, &moving_right(false, false), TICK_DT, &cfg);
        assert!(out2.speed > 0.0, "после стана снова можно двигаться");
    }

    #[test]
    fn apply_stun_refreshes_duration() {
        let cfg = cfg();
        let mut a = Abilities::default();
        a.apply_stun(&cfg);
        // потикали половину
        for _ in 0..((STUN_DURATION * 0.5 / TICK_DT) as usize) {
            tick_abilities(&mut a, &aim_at(0.0), TICK_DT, &cfg);
        }
        let mid = a.stun_left;
        assert!(mid > 0.0 && mid < STUN_DURATION);
        // новый стан сбрасывает таймер в полную длительность
        a.apply_stun(&cfg);
        assert!((a.stun_left - STUN_DURATION).abs() < 1e-4, "новый стан обновил длительность");
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
                want_attack: false,
            want_stun: false,
            },
            0.016,
            &cfg(),
        );
        assert!(
            (fwd.speed - back.speed).abs() < 1e-3,
            "скорость не зависит от направления взгляда"
        );
        let want = MOVE_SPEED;
        assert!((fwd.speed - want).abs() < 1e-3, "обычная скорость = move_speed");
    }

    #[test]
    fn world_speed_is_uniform_across_directions() {
        // МИРОВАЯ скорость монотонна: одинакова во все стороны и равна MOVE_SPEED
        // (без анизотропии/выравнивания под экран).
        let dirs = [
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(-1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
        ];
        for d in dirs {
            let dir = d.normalize();
            let mut a = Abilities::default();
            let out = tick_abilities(
                &mut a,
                &AbilityInput {
                    move_dir: dir,
                    aim: 0.0,
                    want_dash: false,
                    want_block: false,
                    want_attack: false,
            want_stun: false,
                },
                0.016,
                &cfg(),
            );
            assert!(
                (out.speed - MOVE_SPEED).abs() < 1e-2,
                "скорость должна быть MOVE_SPEED во все стороны: {} для {dir:?}",
                out.speed
            );
        }
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
            want_attack: false,
            want_stun: false,
        };
        let out = tick_abilities(&mut b, &inp, 0.016, &cfg());
        assert!(out.did_dash);
        assert_eq!(out.facing, 0.0, "во время рывка модель не разворачивается");
    }
}
