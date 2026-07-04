//! Сетевые сообщения (не реплицируемое состояние, а разовые события/запросы).
//!
//! Stage 1: ядро авторизации и таблицы очков. Боевые FX (MeleeFx/StunFx/DashFx/
//! NpcDied/NpcSound/...) добавим в Stage 10, когда будем переподключать клиентские
//! эффекты/звук к новому стеку.

use bevy::math::Vec2;
use bevy::prelude::Event;
use protocol::messages::{NpcKind, NpcSoundKind, ScoreEntry};
use serde::{Deserialize, Serialize};

/// Запрос авторизации (C2S). Шлётся сразу после соединения, до спавна в мире.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Hello {
    pub name: String,
    pub password: String,
}

/// Авторизация прошла (S2C): сервер заспавнит/привяжет игрока к этому соединению.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthOk;

/// Авторизация отклонена (S2C): неверный пароль / имя уже в игре. После этого
/// сервер закрывает соединение.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthDenied {
    pub reason: String,
}

/// Полная таблица очков (S2C). Шлётся при изменениях (вход/выход/килл/смерть).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Scoreboard(pub Vec<ScoreEntry>);

/// Вид боевого FX-события (для звука/визуала на клиенте).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum FxKind {
    /// Замах мечом (melee).
    Melee,
    /// Удар щитом (стан).
    Stun,
    /// Рывок (dash).
    Dash,
    /// Удар пришёлся в поднятый щит жертвы (урон поглощён) — «дзынь» блока.
    Blocked,
}

/// Чья смерть (для выбора корпса/звука на клиенте).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum DeathKind {
    Player,
    Npc(NpcKind),
}

/// Разовое FX/звуковое событие (S2C). Реплицируемое состояние (позиции/HP) не
/// несёт дискретных «вспышек», поэтому одноразовые эффекты (замах, рывок, взрыв,
/// смерть, рык неписи) сервер шлёт отдельным дешёвым сообщением по `FxChannel`
/// (unreliable). Клиент проигрывает звук/спавнит визуал по координате.
#[derive(Event, Serialize, Deserialize, Clone, Copy, Debug)]
pub enum Fx {
    /// Боевое действие игрока/неписи: замах/щит/рывок из точки `pos` в `dir`.
    Combat { kind: FxKind, pos: Vec2, dir: Vec2 },
    /// Детонация гранаты (взрыв) в точке.
    Detonation { pos: Vec2 },
    /// Смерть сущности (корпс/звук) в точке.
    Death { kind: DeathKind, pos: Vec2 },
    /// Позиционный звук неписи (рык/замах).
    NpcSound { kind: NpcSoundKind, pos: Vec2 },
}
