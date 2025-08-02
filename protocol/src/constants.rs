// Channel IDs
pub const CH_C2S: u8 = 0;
pub const CH_S2C: u8 = 1;

// Timing & movement constants
pub const TICK_DT: f32 = 0.015; // 64Hz
pub const MOVE_SPEED: f32 = 300.0;

// Hit detection
pub const HITBOX_RADIUS: f32 = 20.0;
pub const MAX_RAY_LEN: f32 = 400.0;

// Timeout
pub const TIMEOUT_SECS: f64 = 3.0;

// Respawn
pub const RESPAWN_COOLDOWN: f64 = 5.0;

// Скорость полёта гранаты (пикселей в секунду)
pub const GRENADE_SPEED: f32 = 275.0;
// Время до взрыва
pub const GRENADE_TIMER: f32 = 2.0;
// Радиус взрыва (в тех же единицах, что и мир)
pub const GRENADE_BLAST_RADIUS: f32 = 100.0;