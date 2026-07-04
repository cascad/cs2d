use bevy::prelude::*;
use protocol::messages::ScoreEntry;
use std::collections::{HashMap, VecDeque};

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

/// Желаемый угол взгляда (на курсор), мировой радиан. Пишется `rotate_to_cursor`,
/// читается предсказанием — модель плавно к нему доворачивается (см. `tick_abilities`).
#[derive(Resource, Default)]
pub struct AimAngle(pub f32);

#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

/// Сгенерированная текстура белого круга для рендера игрока (тонируется цветом).
#[derive(Resource, Clone)]
pub struct CircleTex(pub Handle<Image>);

/// Сгенерированный полый ободок (кольцо) — маркер под ногами игрока.
#[derive(Resource, Clone)]
pub struct RingTex(pub Handle<Image>);

/// Сгенерированная стилизованная стрелка-дартик — указатель направления тела.
#[derive(Resource, Clone)]
pub struct ArrowTex(pub Handle<Image>);

#[derive(Resource, Default)]
pub struct HpUiMap(pub HashMap<u64, Entity>);

// Ресурс карты, заполняется на клиенте при загрузке уровня.
#[derive(Resource, Default)]
pub struct SolidTiles(pub std::collections::HashSet<IVec2>);

#[derive(Resource, Default, Clone)]
pub struct SpawnPoints(pub Vec<Vec2>);

#[derive(Resource, Default, Clone)]
pub struct WallAabbCache(pub Vec<(Vec2, Vec2)>);

/// Пространственная сетка стен для быстрых рейкастов (трассеры, обрезка взрывов).
#[derive(Resource, Default)]
pub struct WallGridRes(pub protocol::geom::WallGrid);

/// Сетка препятствий, блокирующих взгляд (стены + глухие колонны).
#[derive(Resource, Default)]
pub struct VisionGridRes(pub protocol::geom::WallGrid);

/// Локальное состояние способностей для UI кулдаунов (из предсказанной сущности).
#[derive(Resource, Default)]
pub struct LocalAbilities(pub protocol::abilities::Abilities);

/// HP/стамина/блок/стан локального игрока из предсказанной сущности (для HUD).
#[derive(Resource)]
pub struct LocalStatus {
    pub stamina: f32,
    pub blocking: bool,
    pub hp: i32,
    pub stun_left: f32,
}

impl Default for LocalStatus {
    fn default() -> Self {
        Self {
            stamina: protocol::constants::STAMINA_MAX,
            blocking: false,
            hp: protocol::constants::PLAYER_MAX_HP,
            stun_left: 0.0,
        }
    }
}

/// Последняя полученная с сервера таблица очков (для оверлея по Tab).
#[derive(Resource, Default)]
pub struct ScoreboardData(pub Vec<ScoreEntry>);
