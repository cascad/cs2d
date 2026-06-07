use bevy::prelude::*;
use protocol::messages::GrenadeEvent;

/// Дискретное событие «игрок погиб»
#[derive(Message)]
pub struct PlayerDied {
    pub victim: u64,
    pub killer: Option<u64>,
}

#[derive(Message)]
pub struct PlayerDamagedEvent {
    pub id: u64,
    pub new_hp: i32,
    pub damage: i32,
}

#[derive(Message)]
pub struct PlayerLeftEvent(pub u64); // id игрока

#[derive(Message, Debug, Clone)]
pub struct GrenadeSpawnEvent(pub GrenadeEvent);

#[derive(Message, Debug, Clone)]
pub struct GrenadeDetonatedEvent {
    pub id: u64,
    pub pos: Vec2,
}