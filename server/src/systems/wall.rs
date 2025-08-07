use bevy::prelude::*;
use bevy::sprite::Sprite;
use protocol::constants::{LEVEL_HEIGHT, LEVEL_WIDTH, TILE_SIZE, WALL_THICKNESS};

#[derive(Component)]
pub struct Wall;

pub fn spawn_level_server(mut commands: Commands) {
    let half_w = LEVEL_WIDTH / 2.0;
    let half_h = LEVEL_HEIGHT / 2.0;
    let wall_color = Color::srgb(0.3, 0.3, 0.35);

    // Левая граница
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
    // Правая
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
    // Верхняя
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
    // Нижняя
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
    // Центральная вертикальная
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
