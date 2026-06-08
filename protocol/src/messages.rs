use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ----- Client → Server -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum C2S {
    Input(InputState),
    Shoot(ShootEvent),
    Heartbeat,
    Goodbye,
    Ping(f64), // отправить метку времени клиента (secs)
    ThrowGrenade(GrenadeEvent),
    Melee(MeleeEvent),
}

// ----- Server → Client -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum S2C {
    Snapshot(WorldSnapshot),
    ShootFx(ShootFx),
    PlayerLeft(u64),
    Pong {
        // ответ сервера
        client_time: f64,
        server_time: f64,
    },
    GrenadeSpawn(GrenadeEvent), // ← спавн гранаты
    PlayerDied {
        victim: u64,
        killer: Option<u64>,
    },
    PlayerRespawn {
        id: u64,
        x: f32,
        y: f32,
    },
    PlayerConnected {
        id: u64,
        x: f32,
        y: f32,
    },
    PlayerDisconnected {
        id: u64,
    },
    PlayerDamaged {
        id: u64,
        new_hp: i32,
        damage: i32,
    },
    GrenadeDetonated {
        id: u64,
        pos: Vec2,
    },
    GrenadeSync { id: u64, pos: Vec2, vel: Vec2, ts: f64 }, // снапшот
    MeleeFx {
        attacker_id: u64,
        from: Vec2,
        dir: Vec2,
    },
    /// Старт рывка (авторитет сервера): клиент проигрывает анимацию переката для
    /// этого игрока в направлении `dir`. Гарантирует, что ролл играется ровно
    /// тогда, когда рывок реально произошёл (без рассинхрона с предсказанием).
    DashFx {
        player_id: u64,
        dir: Vec2,
    },
    /// Непись погибла: клиент проигрывает анимацию смерти (труп) в `facing`.
    /// `kind` задаёт, чьи кадры рисовать (скелет/зомби).
    NpcDied {
        id: u32,
        x: f32,
        y: f32,
        facing: f32,
        kind: NpcKind,
    },
}

/// Тип неписи — определяет набор спрайтов на клиенте. ИИ/логика общие для всех.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NpcKind {
    #[default]
    Skeleton,
    Zombie,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InputState {
    pub seq: u32,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub rotation: f32,
    pub stance: Stance,
    pub timestamp: f64,
    pub block: bool, // удержание блока
    pub dash: bool,  // запрос рывка на этом тике
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootEvent {
    pub shooter_id: u64,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MeleeEvent {
    pub attacker_id: u64,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootFx {
    pub shooter_id: u64,
    pub from: Vec2,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Stance {
    Standing,
    Crouching,
    Prone,
}
impl Default for Stance {
    fn default() -> Self {
        Stance::Standing
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub stance: Stance,
    pub hp: i32,
    pub stamina: f32,
    pub blocking: bool,
}

/// Снапшот НЕПИСЯ (скелета) для клиента: позиция, направление, hp и состояние
/// (идёт ли в атаку — для возможной подсветки/звука; пока просто флаг агра).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NpcSnapshot {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub facing: f32,
    pub hp: i32,
    pub aggro: bool,
    pub kind: NpcKind,
    /// Идёт ли сейчас анимация атаки (для проигрывания удара на клиенте).
    pub attacking: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldSnapshot {
    pub players: Vec<PlayerSnapshot>,
    pub npcs: Vec<NpcSnapshot>,
    pub server_time: f64,
    pub last_input_seq: HashMap<u64, u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrenadeEvent {
    pub id: u64, // уникальный ID гранаты
    pub from: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    pub timer: f32, // время до взрыва
    pub timestamp: f64,
}
