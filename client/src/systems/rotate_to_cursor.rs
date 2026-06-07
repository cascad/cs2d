use crate::components::LocalPlayer;
use crate::render::{pointer_world, WorldPos};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub fn rotate_to_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut player_q: Query<(&WorldPos, &mut Transform), With<LocalPlayer>>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam_q.single() else { return };
    let Ok((wp, mut t)) = player_q.single_mut() else { return };
    let Some(world) = pointer_world(window, camera, cam_tf) else { return };

    let dir = world - wp.0;
    if dir.length_squared() > 0.0 {
        t.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
    }
}
