// Channel IDs
pub const CH_C2S: u8 = 0;
pub const CH_S2C: u8 = 1;

// Timing & movement constants
pub const TICK_DT: f32 = 0.015; // 64Hz
pub const MOVE_SPEED: f32 = 300.0;

// Hit detection, радиус precise при стрельбе
pub const HITBOX_RADIUS: f32 = 20.0;
// от этого зависит обсчет попаданий (на такой дистанции)
// дальность стрельбы
pub const MAX_RAY_LEN: f32 = 3000.0;

// ============================================================================
// Туман войны (серверный куллинг видимости) и защита лаг-компенсации.
// ============================================================================
/// Макс. радиус обзора игрока. Видно всё в этом радиусе, что не закрыто стенами
/// (360°). Обычно больше экрана — задел на будущее. Один и тот же на сервере
/// (куллинг снапшота) и на клиенте (затемнение карты), чтобы не было рассинхрона.
pub const VIEW_RADIUS: f32 = 1000.0;
/// Максимальный откат назад в лаг-компенсации (сек). Клампит присланный клиентом
/// timestamp, чтобы нельзя было «отмотать» произвольно далеко (бэктрек-чит).
pub const MAX_LAG_COMP: f64 = 0.25;

// Timeout
pub const TIMEOUT_SECS: f64 = 3.0;

// Respawn
pub const RESPAWN_COOLDOWN: f64 = 5.0;

// Скорость полёта гранаты (пикселей в секунду)
pub const GRENADE_SPEED: f32 = 300.0;
// Время до взрыва
pub const GRENADE_TIMER: f32 = 2.0;
// Радиус взрыва (в тех же единицах, что и мир)
pub const GRENADE_BLAST_RADIUS: f32 = 200.0;
// secs
pub const GRENADE_USAGE_COOLDOWN: f64 = 2.0;

pub const GRENADE_DAMAGE_COEFF: f32 = 3.0;

pub const SHOOT_RIFLE_DAMAGE: f32 = 20.0;

// размер уровня (по центру, координаты в world space)
pub const LEVEL_WIDTH: f32 = 1200.0;
pub const LEVEL_HEIGHT: f32 = 800.0;
pub const WALL_THICKNESS: f32 = 40.0;

pub const PLAYER_SIZE: f32 = 32.0;

pub const TILE_SIZE: f32 = 32.0;

pub const GRENADE_RADIUS: f32 = 8.0; // визуальный/физический радиус (клиент 16×16)

pub const MAX_STEP: f32 = TILE_SIZE * 0.10; // тонкий подшаг, чтобы не перепрыгивать щели
pub const SEPARATION_EPS: f32 = 0.5; // «волосок», чтобы не залипать после коррекции

// Физика полёта / отскока (должны совпадать на клиенте и сервере)
pub const GRENADE_AIR_DRAG_PER_SEC: f32 = 0.06; // 6%/сек экспоненциально
pub const GRENADE_RESTITUTION: f32    = 0.5;    // упругость отражения
pub const GRENADE_BOUNCE_DAMPING: f32 = 0.70;   // доп. гашение на ударе
pub const GRENADE_STOP_SPEED: f32     = 30.0;   // ниже — считаем, что остановилась

// ============================================================================
// Способности: стамина / ближний бой (melee) / блок / рывок (dash).
// Все значения настраиваемые — это единая точка тюнинга для клиента и сервера.
// ============================================================================

// --- Стамина ---
pub const STAMINA_MAX: f32 = 100.0;
pub const STAMINA_REGEN_PER_SEC: f32 = 22.0; // восстановление, когда не тратим
pub const BLOCK_STAMINA_DRAIN_PER_SEC: f32 = 25.0; // расход при активном блоке
pub const DASH_STAMINA_COST: f32 = 35.0; // разовый расход на рывок
pub const MELEE_STAMINA_COST: f32 = 15.0; // разовый расход на удар

// --- Melee (ближний удар: сектор перед игроком) ---
pub const MELEE_RANGE: f32 = 64.0; // радиус досягаемости
pub const MELEE_HALF_ANGLE: f32 = 0.6; // полу-угол сектора (рад), ~34° в каждую сторону
pub const MELEE_DAMAGE: f32 = 35.0;
pub const MELEE_COOLDOWN: f32 = 0.6; // секунды между ударами

// --- Block (блок: флаг при зажатии + задержка до установки) ---
pub const BLOCK_ESTABLISH_TIME: f32 = 0.25; // сколько держать, пока блок «встанет»
pub const BLOCK_DAMAGE_MULT: f32 = 0.25; // множитель урона при активном блоке
pub const BLOCK_MOVE_MULT: f32 = 0.4; // замедление передвижения при активном блоке

// --- Dash (рывок: ускорение в сторону) ---
pub const DASH_SPEED: f32 = 900.0; // скорость во время рывка
pub const DASH_DURATION: f32 = 0.18; // длительность рывка, сек
pub const DASH_COOLDOWN: f32 = 1.2; // секунды между рывками
