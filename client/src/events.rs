use bevy::prelude::*;

/// Дискретное событие «игрок погиб»
#[derive(Event)]
pub struct PlayerDied {
    pub victim: u64,
    pub killer: Option<u64>,
}