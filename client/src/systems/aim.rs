use bevy::prelude::*;
use crate::components::{AimLineMarker, AimMarker, PlayerMarker};
use crate::render::{layers, pointer_world, world_to_screen, WorldPos};
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
        Query<(&WorldPos, &PlayerMarker)>, // чтение игрока
        Query<(&mut Transform, Option<&AimMarker>, Option<&AimLineMarker>)>, // обновление точки и линии
    )>,
    q_windows: Query<&Window>,
    q_camera: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    let Ok(window) = q_windows.single() else { return; };
    let Ok((camera, cam_tf)) = q_camera.single() else { return; };
    let Some(world_pos) = pointer_world(window, camera, cam_tf) else { return; };

    // Ищем локального игрока
    let players_query = sets.p0();
    let Some(player_pos) = players_query
        .iter()
        .find_map(|(wp, pm)| if pm.0 == me.id { Some(wp.0) } else { None })
    else {
        return;
    };

    let mut dir = world_pos - player_pos;
    let dist = dir.length();

    // Ограничиваем длину луча
    if dist > AIM_MAX_DISTANCE {
        dir = dir.normalize() * AIM_MAX_DISTANCE;
    }

    let aim_pos = player_pos + dir;

    // Обновляем маркер и линию (позиции через границу мир→экран)
    for (mut tf, aim_marker, aim_line) in sets.p1().iter_mut() {
        if aim_marker.is_some() {
            // Красная точка
            let s = world_to_screen(aim_pos);
            tf.translation = Vec3::new(s.x, s.y, layers::AIM);
        } else if aim_line.is_some() {
            // Линия (под изометрию повороты/масштаб ещё придётся пересчитать)
            let len = dir.length();
            let s = world_to_screen(player_pos + dir * 0.5);
            tf.translation = Vec3::new(s.x, s.y, layers::AIM - 1.0);
            tf.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
            tf.scale = Vec3::new(len, 1.0, 1.0); // растягиваем по X
        }
    }
}
