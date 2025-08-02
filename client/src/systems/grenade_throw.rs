use crate::{components::LocalPlayer, resources::MyPlayer, systems::utils::time_in_seconds};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::{
    constants::{CH_C2S, GRENADE_SPEED, GRENADE_TIMER},
    messages::{C2S, GrenadeEvent},
};

pub fn grenade_throw(
    keys: Res<ButtonInput<KeyCode>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    player_query: Query<&Transform, With<LocalPlayer>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
) {
    if !keys.just_pressed(KeyCode::KeyG) {
        return;
    }

    let transform = match player_query.single() {
        Ok(t) => t,
        Err(_) => {
            warn!("🔸 grenade_throw: no LocalPlayer entity found");
            return;
        }
    };
    let player_pos = transform.translation.truncate();

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => {
            warn!("🔸 grenade_throw: no window available");
            return;
        }
    };

    let cursor_screen_pos = match window.cursor_position() {
        Some(p) => p,
        None => {
            warn!("🔸 grenade_throw: cursor not in window");
            return;
        }
    };

    let (camera, cam_transform) = match camera_q.single() {
        Ok(c) => c,
        Err(_) => {
            warn!("🔸 grenade_throw: no camera found");
            return;
        }
    };

    let cursor_world = match camera.viewport_to_world_2d(cam_transform, cursor_screen_pos) {
        Ok(world_pos) => world_pos.trunc(),
        Err(err) => {
            warn!("🔸 grenade_throw: failed to project cursor to world: {err:?}");
            return;
        }
    };

    let mut dir = cursor_world - player_pos;
    if dir.length_squared() <= std::f32::EPSILON {
        warn!("🔸 grenade_throw: zero direction (cursor on player)");
        return;
    }
    dir = dir.normalize();

    let ts = time_in_seconds();
    let ev = GrenadeEvent {
        id: my.id ^ (ts as u64),
        from: player_pos,
        dir,
        speed: GRENADE_SPEED,
        timer: GRENADE_TIMER,
        timestamp: ts,
    };
    if client
        .connection_mut()
        .send_message_on(CH_C2S, C2S::ThrowGrenade(ev.clone()))
        .is_ok()
    {
        info!(
            "💣 Sent ThrowGrenade {}, speed: {}, timer: {}",
            ev.id, ev.speed, ev.timer
        );
    }
}
