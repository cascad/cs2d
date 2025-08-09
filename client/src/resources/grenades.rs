use std::collections::HashMap;

use bevy::prelude::*;
use protocol::messages::GrenadeEvent;

#[derive(Resource)]
pub struct GrenadeCooldown(pub Timer);

impl Default for GrenadeCooldown {
    fn default() -> Self {
        GrenadeCooldown(Timer::from_seconds(2.0, TimerMode::Once))
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