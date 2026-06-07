use crate::components::LocalPlayer;
use crate::render::{pointer_world, WorldPos};
use crate::resources::AimAngle;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Локальный игрок ЦЕЛИТСЯ на курсор: пишем МИРОВОЙ угол на курсор в [`AimAngle`].
/// Сам разворот модели делается плавно в предсказании (`tick_abilities`), а не
/// мгновенно — поэтому `Facing` тут больше не трогаем (его задаёт `send_input`
/// из довёрнутого угла).
pub fn rotate_to_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
    mut aim: ResMut<AimAngle>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam_q.single() else { return };
    let Ok(wp) = player_q.single() else { return };
    let Some(world) = pointer_world(window, camera, cam_tf) else { return };
    let dir = world - wp.0;
    if dir.length_squared() > 0.0 {
        aim.0 = dir.y.atan2(dir.x);
    }
}
