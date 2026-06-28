use bevy::prelude::*;
use bevy_quinnet::client::connection::ConnectionLocalId;
use protocol::messages::{InputState, Stance, WorldSnapshot};
use std::collections::{HashMap, HashSet, VecDeque};

pub mod explosion_textures;
pub mod grenades;

#[derive(Resource)]
pub struct MyPlayer {
    pub id: u64,
    pub got: bool,
}

/// Общий реестр трупов (скелеты + игроки) в порядке появления. Трупы лежат, пока
/// их не больше [`protocol::constants::MAX_CORPSES`]; при превышении удаляется
/// самый старый. Счётчик единый для всех типов трупов.
#[derive(Resource, Default)]
pub struct Corpses(pub VecDeque<Entity>);

impl Corpses {
    /// Регистрирует новый труп; если их стало больше лимита — деспавнит старейший.
    pub fn register(&mut self, commands: &mut Commands, e: Entity) {
        self.0.push_back(e);
        while self.0.len() > protocol::constants::MAX_CORPSES {
            if let Some(old) = self.0.pop_front() {
                if let Ok(mut ec) = commands.get_entity(old) {
                    ec.despawn();
                }
            }
        }
    }
}

#[derive(Resource)]
pub struct TimeSync {
    pub offset: f64,
}

#[derive(Resource)]
pub struct SnapshotBuffer {
    pub snapshots: VecDeque<WorldSnapshot>,
    pub delay: f64,
}

#[derive(Resource)]
pub struct CurrentStance(pub Stance);

/// Желаемый угол взгляда (на курсор), мировой радиан. Пишется `rotate_to_cursor`,
/// читается предсказанием — модель плавно к нему доворачивается (см. `tick_abilities`).
#[derive(Resource, Default)]
pub struct AimAngle(pub f32);

#[derive(Resource)]
pub struct SendTimer(pub Timer);

#[derive(Resource, Default)]
pub struct SpawnedPlayers(pub HashSet<u64>);

#[derive(Resource)]
pub struct SeqCounter(pub u32);

#[derive(Resource, Default)]
pub struct PendingInputsClient(pub VecDeque<InputState>);

#[derive(Resource, Default)]
pub struct CurrentConnId(pub Option<ConnectionLocalId>);

#[derive(Resource)]
pub struct HeartbeatTimer(pub Timer);

impl Default for HeartbeatTimer {
    fn default() -> Self {
        // шлём heartbeat каждую секунду
        HeartbeatTimer(Timer::from_seconds(1.0, TimerMode::Repeating))
    }
}

#[derive(Resource)]
pub struct ClientLatency {
    pub rtt: f64,
    pub offset: f64,  // серверное время = client_time + one_way + offset
    pub timer: Timer, // для пингования
}

impl Default for ClientLatency {
    fn default() -> Self {
        Self {
            rtt: 0.0,
            offset: 0.0,
            timer: Timer::from_seconds(1.0, TimerMode::Repeating),
        }
    }
}

#[derive(Resource, Default)]
pub struct ConnectedPlayers(pub HashSet<u64>);

#[derive(Resource, Default)]
/// Tracks players who are currently “dead” and should _not_ be spawned
pub struct DeadPlayers(pub HashSet<u64>);

#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

/// Сгенерированная текстура белого круга для рендера игрока (тонируется цветом).
#[derive(Resource, Clone)]
pub struct CircleTex(pub Handle<Image>);

/// Сгенерированный полый ободок (кольцо) — маркер под ногами игрока.
#[derive(Resource, Clone)]
pub struct RingTex(pub Handle<Image>);

#[derive(Resource, Default)]
pub struct HpUiMap(pub HashMap<u64, Entity>);

// Ресурс карты, заполняется на клиенте при загрузке уровня (или из сервера).
#[derive(Resource, Default)]
pub struct SolidTiles(pub std::collections::HashSet<IVec2>);

#[derive(Resource, Default, Clone)]
pub struct SpawnPoints(pub Vec<Vec2>);

#[derive(Resource, Default, Clone)]
pub struct WallAabbCache(pub Vec<(Vec2, Vec2)>); // (min, max)

/// Пространственная сетка стен для быстрых рейкастов (трассеры, обрезка взрывов).
#[derive(Resource, Default)]
pub struct WallGridRes(pub protocol::geom::WallGrid);

/// Сетка препятствий, БЛОКИРУЮЩИХ ВЗГЛЯД (стены + глухие колонны, без бочек/
/// сундуков). Используется туманом войны, чтобы за низкими пропами было видно —
/// строго так же, как серверный куллинг (`VisionGridRes` на сервере).
#[derive(Resource, Default)]
pub struct VisionGridRes(pub protocol::geom::WallGrid);

#[derive(Resource, Default)]
pub struct LastKnownPos(pub HashMap<u64, (Vec2, f32)>); // id -> (pos, rot)

/// Время (клиентское, сек) последнего появления игрока в снапшоте. Нужно для
/// плавного скрытия тех, кого сервер перестал присылать (туман войны): пропал
/// из снапшотов → угасает → деспавн. id → last_seen.
#[derive(Resource, Default)]
pub struct LastSeen(pub HashMap<u64, f64>);

/// Локальное предсказание способностей игрока (стамина/рывок/кулдауны).
/// Используется для отзывчивого предсказания рывка; стамина для UI берётся
/// из снапшота (авторитет сервера).
#[derive(Resource, Default)]
pub struct LocalAbilities(pub protocol::abilities::Abilities);

/// Стамина/блок/HP локального игрока из последнего снапшота (для UI).
#[derive(Resource)]
pub struct LocalStatus {
    pub stamina: f32,
    pub blocking: bool,
    pub hp: i32,
}

impl Default for LocalStatus {
    fn default() -> Self {
        Self {
            stamina: protocol::constants::STAMINA_MAX,
            blocking: false,
            hp: protocol::constants::PLAYER_MAX_HP,
        }
    }
}

/// Точная предсказанная позиция локального игрока (симуляция). Двигается в
/// локстепе с отправкой ввода (раз в тик) и правится реконсиляцией. Отрисовка
/// (Transform) плавно тянется к ней — это чисто визуальное сглаживание, на
/// других клиентов не влияет (они видят нас из серверных снапшотов).
#[derive(Resource, Default)]
pub struct PredictedPos {
    /// Авторитетная (предсказанная) позиция, шагает раз в тик + правится реконсиляцией.
    pub pos: Vec2,
    /// Мировая скорость, применённая на последнем тике (move_dir·speed). Отрисовка
    /// интегрирует её КАЖДЫЙ КАДР (экстраполяция) → постоянная, незаметная глазу
    /// скорость, не зависящая ни от FPS, ни от рассинхрона часов «тик vs кадр».
    pub vel: Vec2,
    pub valid: bool,
}

/// Состояние авторизации: для какого client_id уже отправлен `C2S::Hello`.
/// Привязка к id даёт авто-переотправку при реконнекте (новый id ⇒ новый Hello).
#[derive(Resource, Default)]
pub struct AuthState {
    pub sent_for: Option<u64>,
}

/// Последняя полученная с сервера таблица очков (для оверлея по Tab).
#[derive(Resource, Default)]
pub struct ScoreboardData(pub Vec<protocol::messages::ScoreEntry>);
