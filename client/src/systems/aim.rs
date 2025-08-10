use crate::components::{AimMarker, PlayerMarker};
use crate::resources::MyPlayer;
use bevy::prelude::*;

pub const AIM_MAX_DISTANCE: f32 = 500.0; // максимальная длина прицела

pub fn spawn_aim_marker(mut commands: Commands) {
    commands.spawn((
        Sprite {
            color: Color::srgb(1.0, 0.0, 0.0), // красный кружок
            custom_size: Some(Vec2::splat(8.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 1.0),
        GlobalTransform::default(),
        AimMarker,
    ));
}

pub fn update_aim_to_mouse(
    me: Res<MyPlayer>,
    q_players: Query<(&Transform, &PlayerMarker), Without<AimMarker>>,
    mut q_aim: Query<&mut Transform, With<AimMarker>>,
    q_windows: Query<&Window>,
    q_camera: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    let Ok(window) = q_windows.single() else {
        return;
    };
    let Ok((camera, cam_tf)) = q_camera.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_tf, cursor_pos) else {
        return;
    };

    let Some(player_tf) = q_players
        .iter()
        .find_map(|(tf, pm)| if pm.0 == me.id { Some(tf) } else { None })
    else {
        return;
    };

    let player_pos = player_tf.translation.truncate();
    let mut dir = world_pos - player_pos;
    let dist = dir.length();

    if dist > AIM_MAX_DISTANCE {
        dir = dir.normalize() * AIM_MAX_DISTANCE;
    }

    let aim_pos = player_pos + dir;

    for mut tf in &mut q_aim {
        tf.translation = Vec3::new(aim_pos.x, aim_pos.y, 1.0);
    }
}

