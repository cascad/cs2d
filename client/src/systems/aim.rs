use bevy::prelude::*;
use crate::components::{AimLineMarker, AimMarker, PlayerMarker};
use crate::resources::MyPlayer;

pub const AIM_MAX_DISTANCE: f32 = 500.0; // макс. длина прицела

pub fn spawn_aim_marker(mut commands: Commands) {
    // Красная точка
    commands.spawn((
        Sprite {
            color: Color::srgba(1.0, 0.0, 0.0, 1.0),
            custom_size: Some(Vec2::splat(8.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 1.0),
        GlobalTransform::default(),
        AimMarker,
    ));

    // Жёлтая линия
    commands.spawn((
        Sprite {
            color: Color::srgba(1.0, 1.0, 0.0, 0.08),
            custom_size: Some(Vec2::new(1.0, 1.0)), // ширина 2 пикселя
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.5),
        GlobalTransform::default(),
        AimLineMarker,
    ));
}

pub fn update_aim_to_mouse(
    me: Res<MyPlayer>,
    mut sets: ParamSet<(
        Query<(&Transform, &PlayerMarker)>, // чтение игрока
        Query<(&mut Transform, Option<&AimMarker>, Option<&AimLineMarker>)>, // обновление точки и линии
    )>,
    q_windows: Query<&Window>,
    q_camera: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    let Ok(window) = q_windows.single() else { return; };
    let Ok((camera, cam_tf)) = q_camera.single() else { return; };
    let Some(cursor_pos) = window.cursor_position() else { return; };
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_tf, cursor_pos) else { return; };

    // Ищем локального игрока
    let players_query = sets.p0();
    let Some(player_tf) = players_query
        .iter()
        .find_map(|(tf, pm)| if pm.0 == me.id { Some(tf) } else { None })
    else {
        return;
    };

    let player_pos = player_tf.translation.truncate();
    let mut dir = world_pos - player_pos;
    let dist = dir.length();

    // Ограничиваем длину луча
    if dist > AIM_MAX_DISTANCE {
        dir = dir.normalize() * AIM_MAX_DISTANCE;
    }

    let aim_pos = player_pos + dir;

    // Обновляем маркер и линию
    for (mut tf, aim_marker, aim_line) in sets.p1().iter_mut() {
        if aim_marker.is_some() {
            // Красная точка
            tf.translation = Vec3::new(aim_pos.x, aim_pos.y, 1.0);
        } else if aim_line.is_some() {
            // Линия
            let len = dir.length();
            tf.translation = Vec3::new(
                player_pos.x + dir.x * 0.5,
                player_pos.y + dir.y * 0.5,
                0.5,
            );
            tf.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
            tf.scale = Vec3::new(len, 1.0, 1.0); // растягиваем по X
        }
    }
}
