use bevy::prelude::*;
use protocol::messages::NpcSoundKind;

#[derive(Message)]
pub struct PlayerDamagedEvent {
    pub id: u64,
    pub new_hp: i32,
    pub damage: i32,
}

#[derive(Message, Debug, Clone)]
pub struct GrenadeDetonatedEvent {
    pub id: u64,
    pub pos: Vec2,
}

/// Дискретное событие «непись (зомби/скелет) погиб» — для озвучки смерти.
#[derive(Message, Debug, Clone)]
pub struct NpcDiedEvent {
    pub pos: Vec2,
}

/// Позиционный звук неписи (рык/атака) из серверного FX.
#[derive(Message, Debug, Clone)]
pub struct NpcSoundEvent {
    pub kind: NpcSoundKind,
    pub pos: Vec2,
}

/// Боевой звук с позицией (взмах меча, удар щитом-стан, перекат).
#[derive(Message, Debug, Clone)]
pub struct CombatSfxEvent {
    pub kind: CombatSfxKind,
    pub pos: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatSfxKind {
    Swing,
    StunBash,
    Dash,
    /// Удар поглощён поднятым щитом (свой или чужой) — «дзынь» блока.
    Blocked,
}
