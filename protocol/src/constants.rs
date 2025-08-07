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

// Timeout
pub const TIMEOUT_SECS: f64 = 3.0;

// Respawn
pub const RESPAWN_COOLDOWN: f64 = 5.0;

// Скорость полёта гранаты (пикселей в секунду)
pub const GRENADE_SPEED: f32 = 400.0;
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