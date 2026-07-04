//! Общая карта для Lightyear-стека: те же стены/обзор/спавны, что и в старом
//! стеке (из `protocol::maps`). Сетки строятся одним кодом на клиенте и сервере,
//! поэтому коллизия и LOS совпадают до бита (нужно для предсказания без рассинхрона).

use bevy::prelude::*;
use protocol::geom::WallGrid;
use protocol::maps;

/// Размер тайла мира (как в старом `level_fixed.rs`).
pub const TILE: f32 = 32.0;
/// Размер ячейки спатиал-сетки стен (кратен тайлу).
const WALL_GRID_CELL: f32 = TILE * 2.0;

/// Геометрия уровня как ресурс: сетка движения (все стены/пропы), сетка обзора
/// (без низких пропов — для тумана/LOS) и точки спавна.
#[derive(Resource, Clone)]
pub struct MapGrids {
    /// Коллизия движения (скольжение круга игрока/НИП вдоль стен).
    pub movement: WallGrid,
    /// Блокировка взгляда (LOS) — для тумана войны и видимости снапшота.
    pub vision: WallGrid,
    /// Сырые AABB стен движения — для физики гранат (`protocol::grenade`).
    pub wall_aabbs: Vec<(Vec2, Vec2)>,
    /// Точки спавна игроков (мировые центры клеток 'S').
    pub spawns: Vec<Vec2>,
}

impl MapGrids {
    /// Строит карту из общего описания (`protocol::maps::active_level`).
    pub fn load() -> Self {
        let lvl = maps::active_level(TILE);
        let movement = WallGrid::build(&lvl.wall_aabbs, WALL_GRID_CELL);
        let vision = WallGrid::build(&lvl.vision_aabbs, WALL_GRID_CELL);
        Self {
            movement,
            vision,
            wall_aabbs: lvl.wall_aabbs,
            spawns: lvl.spawns,
        }
    }
}

impl Default for MapGrids {
    fn default() -> Self {
        Self::load()
    }
}
