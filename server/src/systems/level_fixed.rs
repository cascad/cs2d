use bevy::prelude::*;

use protocol::geom::WallGrid;
use protocol::maps;

use crate::{
    resources::{SolidTiles, SpawnPoints, WallAabbs, WallGridRes},
    systems::wall::Wall,
};

/// Размер ячейки спатиал-сетки стен (кратен тайлу).
const WALL_GRID_CELL: f32 = TILE * 2.0;

pub const TILE: f32 = 32.0;

/// Построение уровня из ОБЩЕЙ карты (`protocol::maps`): спавнит маркеры стен,
/// возвращает SolidTiles, SpawnPoints и AABB стен. Клиент парсит ту же карту тем
/// же кодом — коллизия и спавны совпадают до бита.
pub fn create_fixed_level(commands: &mut Commands) -> (SolidTiles, Vec<Vec2>, Vec<(Vec2, Vec2)>) {
    let lvl = maps::active_level(TILE);

    // маркеры стен (серверу геометрия нужна для LOS/проверок, отрисовки нет)
    for &(min, max) in &lvl.wall_aabbs {
        let center = (min + max) * 0.5;
        commands.spawn((
            Transform::from_translation(center.extend(0.0)),
            GlobalTransform::default(),
            Wall,
        ));
    }

    let solid = lvl.solid_tiles();
    (SolidTiles(solid), lvl.spawns, lvl.wall_aabbs)
}

/// Системный сетап: один раз строим уровень и кладём ресурсы
pub fn setup_fixed_level(mut commands: Commands) {
    let (solid, spawns, wall_aabbs) = create_fixed_level(&mut commands);

    let grid = WallGrid::build(&wall_aabbs, WALL_GRID_CELL);
    commands.insert_resource(solid);
    commands.insert_resource(SpawnPoints(spawns));
    commands.insert_resource(WallAabbs(wall_aabbs));
    commands.insert_resource(WallGridRes(grid));
}
