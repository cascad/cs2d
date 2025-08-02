use bevy::prelude::*;

/// Дискретное событие «игрок погиб»
#[derive(Event)]
pub struct PlayerDied {
    pub victim: u64,
    pub killer: Option<u64>,
}

#[derive(Event)]
pub struct PlayerDamagedEvent {
    pub id: u64,
    pub new_hp: i32,
    pub damage: i32,
}

#[derive(Event)]
pub struct PlayerLeftEvent(pub u64); // id игрока
