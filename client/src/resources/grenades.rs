use std::collections::HashMap;

use bevy::prelude::*;
use protocol::constants::GRENADE_USAGE_COOLDOWN;
use protocol::messages::GrenadeEvent;

#[derive(Resource)]
pub struct GrenadeCooldown(pub Timer);

impl Default for GrenadeCooldown {
    fn default() -> Self {
        // совпадает с серверным `GRENADE_USAGE_COOLDOWN`, чтобы клиентский rate-limit
        // не расходился с авторитетным КД. Стартуем «готовым»: первый бросок
        // доступен сразу, без ожидания полного таймера.
        let mut t = Timer::from_seconds(GRENADE_USAGE_COOLDOWN as f32, TimerMode::Once);
        let d = t.duration();
        t.set_elapsed(d);
        GrenadeCooldown(t)
    }
}

#[derive(Resource, Default)]
pub struct ClientGrenades(pub HashMap<u64, GrenadeEvent>);

/// Последний снапшот по гранате от сервера
#[derive(Default, Clone, Copy)]
pub struct NetState {
    pub pos: Vec2,
    pub vel: Vec2,
    pub ts:  f64,
    pub has: bool,
}

/// Состояния всех гранат по их id
#[derive(Resource, Default)]
pub struct GrenadeStates(pub HashMap<u64, NetState>);