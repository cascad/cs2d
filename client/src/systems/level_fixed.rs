use bevy::prelude::*;
use bevy::sprite::Anchor;
use protocol::constants::TILE_SIZE;
use protocol::geom::WallGrid;
use protocol::maps::{self, WallMat};

use crate::{
    render::{depth_z, layers, world_to_screen, RenderLayer, WorldPos},
    resources::{SolidTiles, SpawnPoints, WallAabbCache, WallGridRes},
    systems::iso::TILE_ANCHOR_Y,
    systems::level::Wall,
};

pub const TILE: f32 = 32.0;

/// Размеры активной карты (в тайлах): (ширина, высота). Источник — общая
/// `protocol::maps`, чтобы туман и границы камеры совпадали с геометрией.
pub fn map_dims() -> (usize, usize) {
    let lvl = maps::active_level(TILE);
    (lvl.width, lvl.height)
}

/// Оттенок стены по материалу. Пока ассет один (`stoneWall`), кирпич отличаем
/// тёплым тоном — заменится на собственный тайл, когда принесут визуал.
fn wall_tint(mat: WallMat) -> Color {
    match mat {
        WallMat::Stone => Color::srgb(1.0, 1.0, 1.0),
        WallMat::Brick => Color::srgb(1.0, 0.82, 0.68),
    }
}

/// Построение уровня из ОБЩЕЙ карты (`protocol::maps`): считает коллизию (мир) и
/// спавнит изо-визуал стен из пред-рендеренных тайлов kenney.
/// Возвращает (SolidTiles, спавны, AABB стен).
pub fn create_fixed_level(
    commands: &mut Commands,
    asset_server: &AssetServer,
) -> (SolidTiles, Vec<Vec2>, Vec<(Vec2, Vec2)>) {
    let lvl = maps::active_level(TILE);

    // --- изо-визуал стен: блоки kenney `stoneWall`. На каждую стену кладём две
    // грани, обращённые к камере (низ-лево + низ-право) — вместе они дают вид
    // сплошного блока без автотайлинга; задние грани скрыты самим блоком. ---
    let wall_left: Handle<Image> = asset_server.load("iso_env/stoneWall_N.png");
    let wall_right: Handle<Image> = asset_server.load("iso_env/stoneWall_W.png");
    // Стену делаем ВДВОЕ выше (по высоте спрайта), чтобы не выглядела заборчиком.
    // Ширина = ширине ромба клетки (как у пола), поэтому основание совпадает с
    // полом; растягиваем только вертикаль — верх блока поднимается выше.
    // Якорь — доля СОДЕРЖИМОГО (основание ромба), не зависит от высоты спрайта,
    // поэтому при растяжении вертикали основание остаётся на world_to_screen(center),
    // а вверх блок тянется выше.
    const WALL_HEIGHT_SCALE: f32 = 2.0;
    let tile_size = Vec2::new(TILE * 2.0, TILE * 4.0 * WALL_HEIGHT_SCALE);
    let anchor = Anchor(Vec2::new(0.0, TILE_ANCHOR_Y));

    for (center, cell) in &lvl.cells {
        let Some(mat) = cell.wall else { continue };
        let tint = wall_tint(mat);
        let s = world_to_screen(*center);
        // правая грань ниже, левая чуть поверх — чтобы по общему переднему ребру не
        // было z-fight. Сдвиг кодируем в RenderLayer (его читает проекция каждый
        // кадр; иначе z из Transform перетёрся бы). 0.01 ≪ зазора между клетками.
        for (img, layer) in [
            (wall_right.clone(), layers::WALL),
            (wall_left.clone(), layers::WALL + 0.01),
        ] {
            commands.spawn((
                Sprite {
                    image: img,
                    color: tint,
                    custom_size: Some(tile_size),
                    ..default()
                },
                anchor,
                Transform::from_xyz(s.x, s.y, depth_z(*center, layer)),
                GlobalTransform::default(),
                WorldPos(*center),
                RenderLayer(layer),
                Wall,
            ));
        }
    }

    (SolidTiles(lvl.solid_tiles()), lvl.spawns, lvl.wall_aabbs)
}

/// Системный сетап: один раз строим уровень и кладём ресурсы. Спатиал-сетка
/// стен (`WallGrid`) и кэш AABB строятся СРАЗУ из мировых данных.
pub fn setup_fixed_level(mut commands: Commands, asset_server: Res<AssetServer>) {
    let (solid, spawns, wall_aabbs) = create_fixed_level(&mut commands, &asset_server);

    let grid = WallGrid::build(&wall_aabbs, TILE_SIZE * 2.0);
    commands.insert_resource(solid);
    commands.insert_resource(SpawnPoints(spawns));
    commands.insert_resource(WallGridRes(grid));
    commands.insert_resource(WallAabbCache(wall_aabbs));
}
