use bevy::prelude::*;
use protocol::constants::{LEVEL_HEIGHT, LEVEL_WIDTH, TILE_SIZE, WALL_THICKNESS};

use crate::resources::SolidTiles;

#[derive(Component)]
pub struct Wall;

pub fn spawn_level_client(mut commands: Commands) {
    let half_w = LEVEL_WIDTH / 2.0;
    let half_h = LEVEL_HEIGHT / 2.0;
    let wall_color = Color::srgb(0.3, 0.3, 0.35);

    // 4 границы
    commands.spawn((
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                WALL_THICKNESS,
                LEVEL_HEIGHT + WALL_THICKNESS * 2.0,
            )),
            ..default()
        },
        Transform::from_translation(Vec3::new(-half_w - WALL_THICKNESS / 2.0, 0.0, 0.0)),
        GlobalTransform::default(),
        Wall,
    ));
    commands.spawn((
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                WALL_THICKNESS,
                LEVEL_HEIGHT + WALL_THICKNESS * 2.0,
            )),
            ..default()
        },
        Transform::from_translation(Vec3::new(half_w + WALL_THICKNESS / 2.0, 0.0, 0.0)),
        GlobalTransform::default(),
        Wall,
    ));
    commands.spawn((
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                LEVEL_WIDTH + WALL_THICKNESS * 2.0,
                WALL_THICKNESS,
            )),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, half_h + WALL_THICKNESS / 2.0, 0.0)),
        GlobalTransform::default(),
        Wall,
    ));
    commands.spawn((
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                LEVEL_WIDTH + WALL_THICKNESS * 2.0,
                WALL_THICKNESS,
            )),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, -half_h - WALL_THICKNESS / 2.0, 0.0)),
        GlobalTransform::default(),
        Wall,
    ));
    // центральная
    commands.spawn((
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, LEVEL_HEIGHT * 0.8)),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
        GlobalTransform::default(),
        Wall,
    ));
}

pub fn fill_solid_tiles_once(
    mut solids: ResMut<SolidTiles>,
    q_walls: Query<(&Transform, &Sprite), With<Wall>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }

    for (tf, sprite) in q_walls.iter() {
        // Размер стены (custom_size всегда Some у твоих спрайтов)
        if let Some(size) = sprite.custom_size {
            let half = size / 2.0;

            // Мин/макс углы прямоугольника в мировых координатах
            let min = tf.translation.truncate() - half;
            let max = tf.translation.truncate() + half;

            // Диапазон тайлов, которые занимает стена
            let tx_min = (min.x / TILE_SIZE).floor() as i32;
            let tx_max = (max.x / TILE_SIZE).floor() as i32;
            let ty_min = (min.y / TILE_SIZE).floor() as i32;
            let ty_max = (max.y / TILE_SIZE).floor() as i32;

            for tx in tx_min..=tx_max {
                for ty in ty_min..=ty_max {
                    solids.0.insert(IVec2::new(tx, ty));
                }
            }
        }
    }

    info!(target: "collision", "SolidTiles filled: {}", solids.0.len());
    *done = true;
}
