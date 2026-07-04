use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ----- Client → Server -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum C2S {
    /// Авторизация: имя аккаунта + пароль. Шлётся сразу после установления
    /// соединения, ДО спавна в мире. Сервер либо регистрирует имя при первом
    /// входе, либо проверяет пароль. До успешной авторизации остальные C2S
    /// (Input/Shoot/…) игнорируются, а игрок не появляется на карте.
    Hello { name: String, password: String },
    Input(InputState),
    Shoot(ShootEvent),
    Heartbeat,
    Goodbye,
    Ping(f64), // отправить метку времени клиента (secs)
    ThrowGrenade(GrenadeEvent),
}

// ----- Server → Client -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum S2C {
    /// Авторизация прошла: сервер заспавнил игрока, `your_id` — его id в мире.
    AuthOk { your_id: u64 },
    /// Авторизация отклонена (неверный пароль / имя уже в игре). После этого
    /// сервер закрывает соединение; клиент показывает причину и уходит в меню.
    AuthDenied { reason: String },
    /// Полная таблица очков (как в CS). Шлётся при изменениях (вход/выход/килл/
    /// смерть). Накапливается по аккаунту и переживает реконнекты, пока жив сервер.
    Scoreboard(Vec<ScoreEntry>),
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
    /// Удар щитом (stun) стартовал: клиент проигрывает анимацию Kick у атакующего
    /// в направлении `dir`. Урона нет; само оглушение цели приходит через
    /// `stun_left` в снапшоте (авторитет сервера).
    StunFx {
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
    /// Позиционный звук неписи (рык/атака), который слышно даже за стеной в
    /// пределах `NPC_AUDIO_RADIUS` (в обход куллинга видимости). Клиент сам
    /// затухает громкость по дистанции до своего игрока, чтобы на слух
    /// оценивать, далеко ли зомби. Шлётся только тем, кто в радиусе слышимости.
    NpcSound {
        kind: NpcSoundKind,
        x: f32,
        y: f32,
    },
}

/// Категория позиционного звука неписи. Клиент выбирает пул клипов по ней.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcSoundKind {
    /// Амбиентный рык/стон зомби-скелета.
    Growl,
    /// Замах/удар неписи (звук атаки).
    Attack,
}

/// Одна строка таблицы очков. Привязана к АККАУНТУ (имени), а не к соединению,
/// поэтому статистика сохраняется при реконнекте. `id` — текущий client_id
/// (0, если игрок сейчас оффлайн), `online` — в игре ли он прямо сейчас.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScoreEntry {
    pub id: u64,
    pub name: String,
    pub kills: u32,     // убийства других игроков
    pub npc_kills: u32, // убитые непись (скелеты/зомби)
    pub deaths: u32,    // смерти (вкл. выход из игры)
    pub online: bool,
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
    pub block: bool,  // удержание блока
    pub dash: bool,   // запрос рывка на этом тике
    pub attack: bool, // запрос ближнего удара на этом тике
    pub stun: bool,   // запрос удара щитом (Q) на этом тике
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootEvent {
    pub shooter_id: u64,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Авторитетный кулдаун ближнего удара (сек до готовности) ЛОКАЛЬНОГО игрока.
    /// Клиент сидирует им предсказание и реконсилит свой КД — иначе два счётчика
    /// (клиент/сервер) расходятся на ±1 тик и второй удар «теряется».
    pub melee_cd_left: f32,
    /// Авторитетный кулдаун рывка (сек до готовности) ЛОКАЛЬНОГО игрока. Тот же
    /// приём, что и для `melee_cd_left`: клиент сидирует/реконсилит свой КД рывка,
    /// иначе таймеры расходятся и сервер выполняет рывок, который клиент не
    /// предсказал (двигает игрока без анимации переката).
    pub dash_cd_left: f32,
    /// Остаток оглушения (сек). Для ЛОКАЛЬНОГО игрока клиент сидирует им
    /// предсказание (замирает синхронно с сервером); для остальных и неписей —
    /// рисует «звёздочки» над головой, пока >0.
    pub stun_left: f32,
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
    /// Остаток оглушения неписи (сек): пока >0 — рисуем «звёздочки» над головой,
    /// непись бездействует.
    pub stun_left: f32,
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
    /// Желаемая точка падения (куда указал курсор). Сервер клампит её до
    /// максимальной дальности и взрывает банку при достижении (или по таймеру).
    pub target: Vec2,
    pub speed: f32,
    pub timer: f32, // время до взрыва
    pub timestamp: f64,
}
