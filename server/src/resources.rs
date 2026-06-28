use bevy::{
    math::{IVec2, Vec2},
    prelude::{Resource, Timer, TimerMode},
};
use protocol::{
    constants::RESPAWN_COOLDOWN,
    messages::{GrenadeEvent, InputState},
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Default, Clone)]
pub struct PlayerState {
    pub pos: Vec2,
    pub rot: f32,
    /// Желаемый угол (куда смотрит КУРСОР) — мгновенный, без плавного доворота
    /// модели. Используется для блока и поля зрения: игрок «смотрит» туда, куда
    /// целится, а не куда уже довернулась модель.
    pub aim: f32,
    pub stance: protocol::messages::Stance,
    pub hp: i32,
    pub abilities: protocol::abilities::Abilities,
    pub blocking: bool,
    /// Последний полученный ввод. Если на тик сервера не пришёл свежий пакет
    /// (сеть/джиттер дают «пустые» тики), переиспользуем его — иначе блок/ходьба
    /// «дёргались» бы, а блок сбрасывался и удары проходили.
    pub last_input: Option<protocol::messages::InputState>,
}

#[derive(Resource, Default)]
pub struct PlayerStates(pub HashMap<u64, PlayerState>);

/// Запланированный (но ещё не нанесённый) удар ближнего боя. Урон считается не
/// мгновенно по клику, а в СЕРЕДИНЕ анимации (`resolve_at`), по позициям целей
/// на этот момент — так попадание совпадает с визуальным взмахом.
pub struct PendingMelee {
    pub attacker: u64,
    pub dir: Vec2,
    pub resolve_at: f64,
}

#[derive(Resource, Default)]
pub struct PendingMelees(pub Vec<PendingMelee>);

#[derive(Resource)]
pub struct ServerTickTimer(pub Timer);

#[derive(Resource)]
pub struct SnapshotHistory {
    pub buf: VecDeque<(f64, HashMap<u64, PlayerState>)>,
    pub cap: usize,
}
impl Default for SnapshotHistory {
    fn default() -> Self {
        Self {
            buf: VecDeque::with_capacity(120),
            cap: 120,
        }
    }
}

#[derive(Resource, Default)]
pub struct PendingInputs(pub HashMap<u64, VecDeque<InputState>>);

#[derive(Resource, Default)]
pub struct AppliedSeqs(pub HashMap<u64, u32>);

#[derive(Resource, Default)]
pub struct LastHeard(pub HashMap<u64, f64>); // client_id → time (secs)

pub struct GrenadeState {
    pub ev: GrenadeEvent,
    pub created: f64,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Сколько раз снаряд уже отскочил от стены. Достигнув `GRENADE_MAX_BOUNCES`,
    /// при следующем ударе о стену разбивается (детонация на месте контакта).
    pub bounces: u32,
    /// Предельная длина пути (мир. ед.) от точки спавна: банка взрывается, когда
    /// пройденный путь достигнет лимита (игрок указал точку ближе максимума).
    /// Накапливается по реальной траектории — корректно и при будущих отскоках.
    pub travel_limit: f32,
    pub traveled: f32,
}

#[derive(Resource, Default)]
pub struct Grenades(pub HashMap<u64, GrenadeState>);

#[derive(Resource)]
pub struct RespawnDelay(pub f64); // секунды до респавна
impl Default for RespawnDelay {
    fn default() -> Self {
        RespawnDelay(RESPAWN_COOLDOWN)
    }
}

#[derive(Clone)]
pub struct RespawnTask {
    pub pid: u64,
    pub due: f64,  // абсолютное время (сек) когда респавнить
    pub pos: Vec2, // куда ставить
}

#[derive(Resource, Default)]
pub struct RespawnQueue(pub Vec<RespawnTask>);

#[derive(Resource, Default)]
pub struct ConnectedClients(pub HashSet<u64>);

#[derive(Resource, Default)]
pub struct SpawnedClients(pub HashSet<u64>);

#[derive(Resource, Default)]
pub struct LastGrenadeThrows {
    pub map: HashMap<u64, f64>, // client_id → last throw time
}

#[derive(Resource, Default)]
pub struct GrenadeSyncTimer(pub Timer);

// Ресурс карты, заполняется на клиенте при загрузке уровня (или из сервера).
#[derive(Resource, Default)]
pub struct SolidTiles(pub std::collections::HashSet<IVec2>);

/// Предрасчитанные AABB всех стен (min, max) в мировых координатах.
/// Строится один раз при загрузке уровня и используется всеми
/// серверными проверками коллизий вместо перебора ECS-запроса по стенам.
#[derive(Resource, Default)]
pub struct WallAabbs(pub Vec<(Vec2, Vec2)>);

/// Пространственная сетка стен для быстрых отрезковых запросов (рейкаст/LOS).
#[derive(Resource, Default)]
pub struct WallGridRes(pub protocol::geom::WallGrid);

/// Сетка препятствий, БЛОКИРУЮЩИХ ВЗГЛЯД (стены + глухие пропы-колонны, без
/// бочек/сундуков). Используется ТОЛЬКО для куллинга видимости в снапшоте, чтобы
/// за низкими пропами врага было видно, а движение по-прежнему блокировалось.
#[derive(Resource, Default)]
pub struct VisionGridRes(pub protocol::geom::WallGrid);

#[derive(Resource, Default, Clone)]
pub struct SpawnPoints(pub Vec<Vec2>);

// ── НЕПИСИ (скелеты) ────────────────────────────────────────────────────────
/// Режим ИИ скелета.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NpcMode {
    /// бродит по патрульному маршруту
    Wander,
    /// преследует игрока (видит сейчас или недавно видел)
    Chase,
    /// возвращается к маршруту после потери игрока
    Return,
}

#[derive(Clone)]
pub struct Npc {
    pub pos: Vec2,
    pub facing: f32,
    pub hp: i32,
    pub kind: protocol::messages::NpcKind,
    pub mode: NpcMode,
    pub route: usize,
    pub wp: usize,
    pub wp_dir: i32,
    pub target_pos: Vec2,
    pub lost_timer: f32,
    pub attack_cd: f32,
    /// Сколько ещё секунд проигрывается анимация атаки (для снапшота клиенту).
    pub attack_anim: f32,
    pub home: Vec2,
}

#[derive(Resource, Default)]
pub struct Npcs(pub HashMap<u32, Npc>);

#[derive(Resource, Default)]
pub struct NpcIdCounter(pub u32);

/// Патрульные маршруты с карты (мировые точки).
#[derive(Resource, Default, Clone)]
pub struct NpcRoutes(pub Vec<Vec<Vec2>>);

/// Точки автоспавна скелетов (мировые).
#[derive(Resource, Default, Clone)]
pub struct NpcSpawnPoints(pub Vec<Vec2>);

/// «Подсветка» атакующих жертве: viewer → (атакующий → время истечения, сек).
/// Пока не истекло, атакующий шлётся жертве в снапшоте В ОБХОД поля зрения, даже
/// если бьёт сбоку/сзади (вне обзора) — чтобы жертва видела, кто её бьёт.
#[derive(Resource, Default)]
pub struct Reveals {
    pub players: HashMap<u64, HashMap<u64, f64>>,
    pub npcs: HashMap<u64, HashMap<u32, f64>>,
}

#[derive(Resource)]
pub struct NpcRespawnTimer(pub Timer);
impl Default for NpcRespawnTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(
            protocol::constants::NPC_RESPAWN_TIME,
            TimerMode::Repeating,
        ))
    }
}
