use bevy::prelude::*;

/// Событие урона: любой источник пишет сюда
#[derive(Event)]
pub struct DamageEvent {
    pub target: u64,
    pub amount: i32,
    pub source: Option<u64>,
}

#[derive(Event)]
pub struct ClientConnected(pub u64);

#[derive(Event)]
pub struct ClientDisconnected(pub u64);

// Дискретное событие «игрок должен появиться»
#[derive(Event)]
pub struct PlayerRespawn {
    pub id: u64,
    pub x: f32,
    pub y: f32,
}
