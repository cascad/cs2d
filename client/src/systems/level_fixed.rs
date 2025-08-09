use bevy::prelude::*;
use std::collections::HashSet;

use crate::{
    resources::{SolidTiles, SpawnPoints}, systems::level::Wall,
};

pub const TILE: f32 = 32.0;

/// Хардкодная карта: '#' — стена, '.' — пусто, 'S' — спавн-поинт
fn map_lines() -> &'static [&'static str] {
    &[
        // 48x22, ОЧЕНЬ просторная арена, без узких коридоров. Широкие проходы, несколько
        // горизонтальных барьеров. Спавны (S) стоят в больших залах, вдали от стен.
        "################################################",
        "#..............................................#",
        "#..............................................#",
        "#......S......................................#",
        "#...........................................S..#",
        "#..............##############.................#",
        "#..............................................#",
        "#..............................................#",
        "#................###############..............#",
        "#..............................................#",
        "#..............................................#",
        "#..............................................#",
        "#..............#############..................#",
        "#..............................................#",
        "#....S.........................................#",
        "#..............................................#",
        "#..................##############.............#",
        "#..............................................#",
        "#..............................................#",
        "#..............................................#",
        "#..............................................#",
        "################################################",
    ]
}


/// Построение уровня: спавнит стены, возвращает SolidTiles и SpawnPoints
pub fn create_fixed_level(commands: &mut Commands) -> (SolidTiles, Vec<Vec2>) {
    let lines = map_lines();
    let h = lines.len() as i32;
    let w = lines[0].len() as i32;

    let mut solid: HashSet<IVec2> = HashSet::new();
    let mut spawns: Vec<Vec2> = Vec::new();

    // сделаем (0,0) по центру карты
    let origin = Vec2::new(-(w as f32) * TILE * 0.5, -(h as f32) * TILE * 0.5);

    for (jy, row) in lines.iter().enumerate() {
        let y = jy as i32;
        for (jx, ch) in row.chars().enumerate() {
            let x = jx as i32;
            let world_xy = origin + Vec2::new((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);

            match ch {
                '#' => {
                    solid.insert(IVec2::new(x, y));
                    // стена (прямоугольник TILE x TILE)
                    commands.spawn((
                        Sprite {
                            color: Color::srgba(0.25, 0.28, 0.34, 1.0),
                            custom_size: Some(Vec2::splat(TILE)),
                            ..default()
                        },
                        Transform::from_translation(world_xy.extend(0.0)),
                        GlobalTransform::default(),
                        Wall, // твой маркер стены
                    ));
                }
                'S' => {
                    spawns.push(world_xy);
                }
                '.' | _ => {}
            }
        }
    }

    return (SolidTiles(solid), spawns);
}

/// Системный сетап: один раз строим уровень и кладём ресурсы
pub fn setup_fixed_level(mut commands: Commands) {
    let (solid, spawns) = create_fixed_level(&mut commands);

    commands.insert_resource(solid);
    commands.insert_resource(SpawnPoints(spawns));
}
