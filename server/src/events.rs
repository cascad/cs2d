use bevy::prelude::*;

/// Событие урона ИГРОКУ: любой источник пишет сюда
#[derive(Message)]
pub struct DamageEvent {
    pub target: u64,
    pub amount: i32,
    pub source: Option<u64>,
    /// Позиция источника урона для направленного блока. Если `None` — берётся из
    /// состояния игрока-источника (`source`). Непись-атакующий передаёт свою точку
    /// здесь (его нет в `PlayerStates`).
    pub source_pos: Option<bevy::math::Vec2>,
    /// Если урон нанёс непись (скелет) — его id, чтобы «подсветить» его жертве.
    pub npc_source: Option<u32>,
}

/// Событие урона НЕПИСЮ (скелету) — от игрока (ближний бой/выстрел).
#[derive(Message)]
pub struct NpcDamageEvent {
    pub target: u32,
    pub amount: i32,
}

#[derive(Message)]
pub struct ClientConnected(pub u64);

#[derive(Message)]
pub struct ClientDisconnected(pub u64);

// Дискретное событие «игрок должен появиться»
#[derive(Message)]
pub struct PlayerRespawn {
    pub id: u64,
    pub x: f32,
    pub y: f32,
}
