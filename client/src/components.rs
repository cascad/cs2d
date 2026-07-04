use bevy::prelude::*;

#[derive(Component)]
pub struct LocalPlayer;

#[derive(Component)]
pub struct PlayerMarker(pub u64);

/// Направление взгляда актёра в МИРОВЫХ радианах (как `rotation` в снапшоте/вводе).
/// Спрайт-направление (1 из 16) выбирается из ЭКРАННОЙ проекции этого угла, чтобы
/// персонаж смотрел туда же, куда курсор на экране. Заменяет `Transform.rotation`
/// для отрисовки (направленные спрайты не вращают сам quad).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Facing(pub f32);

/// Состояние анимации актёра (направленный спрайт-лист рыцаря).
/// `Idle`/`Walk` зациклены и выбираются по факту движения; `Attack`/`Dash` —
/// одноразовые (oneshot): проигрываются один раз и возвращаются к idle/walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimState {
    Idle,
    Walk,
    Attack,
    Dash,
    Block,
    /// Удар щитом (stun) — одноразовая анимация Kick.png.
    Kick,
    /// Получение урона — одноразовая анимация TakeDamage.png.
    Hurt,
    /// Бросок зелья/гранаты — одноразовая анимация CastSpell.png.
    Cast,
}

impl AnimState {
    /// Одноразовая ли анимация (играется один раз, потом возврат к idle/walk).
    pub fn is_oneshot(self) -> bool {
        matches!(
            self,
            AnimState::Attack | AnimState::Dash | AnimState::Kick | AnimState::Hurt | AnimState::Cast
        )
    }
}

/// Покадровая анимация направленного актёра (рыцарь, лист 8 направлений × N кадров).
/// Кадр и таймер общие для всех направлений; строка листа (направление) выбирается
/// каждый кадр из [`Facing`].
#[derive(Component)]
pub struct ActorAnim {
    pub state: AnimState,
    pub frame: usize,
    pub timer: Timer,
    /// Предыдущая мировая позиция — чтобы отличать ходьбу от покоя.
    pub prev: Vec2,
    /// Если задано — на время одноразового действия повернуть спрайт в этот
    /// МИРОВОЙ угол (рад), а не на курсор. Нужно для рывка: рыцарь катится туда,
    /// КУДА летит, а не куда целится.
    pub lock_facing: Option<f32>,
    /// Держим ли блок сейчас (mouse2 установлен). Перебивает idle/walk на
    /// анимацию блока, пока не идёт одноразовое действие.
    pub blocking: bool,
    /// Остаток времени «вспышки урона» (сек): модель краснеет при получении урона
    /// и плавно возвращается к норме. Видно и на себе, и на других игроках.
    pub hit_flash: f32,
    /// Остаток оглушения (сек) для ЭТОГО актёра (из снапшота). Пока >0 — модель
    /// замирает в Idle и над головой горят «звёздочки».
    pub stun_left: f32,
    /// Сглаженная скорость (мир. ед./СЕК): решение «идёт/стоит» без дребезга
    /// Walk↔Idle на кадрах, где FixedUpdate не тикнул (частота кадров ≠ тикрейт).
    pub move_ema: f32,
}

impl ActorAnim {
    /// Запустить одноразовое действие (атака/рывок): с нулевого кадра.
    pub fn start_action(&mut self, state: AnimState) {
        self.state = state;
        self.frame = 0;
        self.timer.reset();
        self.lock_facing = None;
    }

    /// Запустить одноразовое действие с фиксированным направлением спрайта
    /// (мировой угол, рад) — например рывок в сторону движения.
    pub fn start_action_facing(&mut self, state: AnimState, world_angle: f32) {
        self.start_action(state);
        self.lock_facing = Some(world_angle);
    }
}

impl Default for ActorAnim {
    fn default() -> Self {
        Self {
            state: AnimState::Idle,
            frame: 0,
            timer: Timer::from_seconds(1.0 / 12.0, TimerMode::Repeating),
            prev: Vec2::ZERO,
            lock_facing: None,
            blocking: false,
            hit_flash: 0.0,
            stun_left: 0.0,
            move_ema: 0.0,
        }
    }
}

#[derive(Component)]
pub struct Bullet {
    pub ttl: f32,
    pub vel: Vec2,
}

#[derive(Component)]
pub struct Health(pub i32);

/// Заливка плавающей полоски HP (ребёнок UI-узла над игроком).
#[derive(Component)]
pub struct HpFill {
    pub id: u64,
    pub full_w: f32,
}

/// Компонент «летящая граната»
#[derive(Component)]
pub struct Grenade {
    pub id: u64,
    pub from: Vec2,
    /// Нормализованное направление полёта
    pub dir: Vec2,
    /// Скорость (пикселей в секунду)
    pub speed: f32,
    /// Таймер до взрыва
    pub timer: Timer,
    /// Радиус взрыва (в тех же единицах, что и мир)
    pub blast_radius: f32,
}

#[derive(Component)]
/// Эффект взрыва
pub struct Explosion {
    /// Таймер длительности эффекта
    pub timer: Timer,
}

/// Анимированный слой взрыва (наземная волна / огненный шар / вспышка). За время
/// жизни масштабируется `scale_from→scale_to`, поднимается по экрану на `rise`
/// пикселей и плавно гаснет (альфа `alpha_from→0`). Так плоский круг превращается
/// в «объёмный» всплеск. Альфу пишем в материал из [`ExplosionMaterial`].
#[derive(Component)]
pub struct ExplosionFx {
    pub timer: Timer,
    /// Базовая экранная позиция (с учётом Z-слоя), от неё считаем подъём.
    pub base: Vec3,
    pub scale_from: f32,
    pub scale_to: f32,
    /// Подъём по экрану (px) за всю жизнь — даёт ощущение «вверх».
    pub rise: f32,
    pub alpha_from: f32,
}

/// Компонент на визуальной гранате: связывает сущность с id гранаты на сервере
#[derive(Component)]
pub struct GrenadeNet {
    pub id: u64,
}

#[derive(Component)]
pub struct Corpse {
    pub row: usize,    // направление (ряд листа) — как смотрел рыцарь в момент смерти
    pub frame: usize,  // текущий кадр анимации смерти (доходит до конца и держится)
    pub anim: Timer,   // покадровый таймер проигрывания
    // срок жизни не задаём: труп лежит, пока не вытеснен лимитом MAX_CORPSES.
}

/// Маркер сущности НЕПИСЯ (скелета) с его серверным id.
#[derive(Component)]
pub struct NpcMarker(pub u32);

/// Тип неписи на клиенте (какой набор спрайтов рисовать).
#[derive(Component, Clone, Copy)]
pub struct NpcKindC(pub protocol::messages::NpcKind);

/// Покадровая анимация скелета (8 направлений × 8 кадров отдельными PNG).
#[derive(Component)]
pub struct NpcAnim {
    pub frame: usize,
    pub timer: Timer,
    /// Предыдущая мировая позиция — чтобы крутить ходьбу ТОЛЬКО когда реально
    /// движется (иначе «марширует на месте», когда снапшоты замерли).
    pub prev: Vec2,
    /// Сглаженная скорость (мир. ед./СЕК) — меньше дёрганья от сетевых скачков.
    pub move_ema: f32,
    /// Локальный таймер атаки (сек): стартует по FX/фронту, не ждём реплику.
    pub attack_left: f32,
}

impl Default for NpcAnim {
    fn default() -> Self {
        Self {
            frame: 0,
            timer: Timer::from_seconds(1.0 / 10.0, TimerMode::Repeating),
            prev: Vec2::ZERO,
            move_ema: 0.0,
            attack_left: 0.0,
        }
    }
}

/// Заливка полоски HP над скелетом (ребёнок сущности неписи).
#[derive(Component)]
pub struct NpcHpFill {
    pub id: u32,
    pub full_w: f32,
}

// компонент для маркера
#[derive(Component)]
pub struct AimMarker;

/// Засечка-указатель направления на кольце под игроком (ребёнок сущности игрока).
/// Позиция на изо-эллипсе пересчитывается каждый кадр из [`Facing`] родителя.
#[derive(Component)]
pub struct DirNotch;

#[derive(Component)]
pub struct AimLineMarker;

/// «Звёздочки» оглушения над головой актёра (игрока/неписи). Это ОТДЕЛЬНАЯ
/// мировая `Text2d`-сущность (как всплывающий урон — такой рендер точно работает,
/// в отличие от дочернего текста под спрайтом), которая каждый кадр следует за
/// целью `target`, пока та оглушена; когда стан спадает или цель исчезает — гаснет.
#[derive(Component)]
pub struct StunStars {
    pub target: Entity,
}