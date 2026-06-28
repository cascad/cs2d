use bevy::prelude::*;
use protocol::messages::{GrenadeEvent, NpcSoundKind};

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

/// Дискретное событие «непись (зомби/скелет) погиб» — для озвучки смерти.
#[derive(Message, Debug, Clone)]
pub struct NpcDiedEvent {
    pub pos: Vec2,
}

/// Позиционный звук неписи (рык/атака) из серверного `S2C::NpcSound`. Громкость
/// клиент затухает по дистанции от своего игрока (слышно даже за стеной).
#[derive(Message, Debug, Clone)]
pub struct NpcSoundEvent {
    pub kind: NpcSoundKind,
    pub pos: Vec2,
}

/// Боевой звук с позицией (взмах меча, удар щитом-стан, перекат). Для СВОИХ
/// действий шлётся с позицией локального игрока (звучит в полную громкость), для
/// чужих — с позицией актёра (затухает по дистанции).
#[derive(Message, Debug, Clone)]
pub struct CombatSfxEvent {
    pub kind: CombatSfxKind,
    pub pos: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatSfxKind {
    /// Взмах меча (вне зависимости от попадания).
    Swing,
    /// Удар щитом (стан).
    StunBash,
    /// Перекат (рывок).
    Dash,
}