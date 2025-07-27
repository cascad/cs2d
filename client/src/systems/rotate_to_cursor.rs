use crate::components::LocalPlayer;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub fn rotate_to_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut player_q: Query<&mut Transform, With<LocalPlayer>>,
) {
    if let Ok(window) = windows.single() {
        if let Ok((camera, cam_tf)) = cam_q.single() {
            if let Ok(mut t) = player_q.single_mut() {
                if let Some(cursor) = window.cursor_position() {
                    if let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor) {
                        let dir = world - t.translation.truncate();
                        if dir.length_squared() > 0.0 {
                            t.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
                        }
                    }
                }
            }
        }
    }
}
