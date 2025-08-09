use crate::{
    components::LocalPlayer,
    resources::{MyPlayer, grenades::GrenadeCooldown},
    systems::utils::time_in_seconds,
};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::{
    constants::{CH_C2S, GRENADE_RADIUS, GRENADE_SPEED, GRENADE_TIMER},
    messages::{GrenadeEvent, C2S},
};

pub fn grenade_throw(
    keys: Res<ButtonInput<KeyCode>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    player_query: Query<&Transform, With<LocalPlayer>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    mut grenade_cd: ResMut<GrenadeCooldown>,
    time: Res<Time>,
) {
    grenade_cd.0.tick(time.delta());

    if !keys.just_pressed(KeyCode::KeyG) || !grenade_cd.0.finished() {
        return;
    }

    let transform = match player_query.single() {
        Ok(t) => t,
        Err(_) => return,
    };
    let player_pos = transform.translation.truncate();

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };

    let cursor_screen_pos = match window.cursor_position() {
        Some(p) => p,
        None => return,
    };

    let (camera, cam_transform) = match camera_q.single() {
        Ok(c) => c,
        Err(_) => return,
    };

    let cursor_world = match camera.viewport_to_world_2d(cam_transform, cursor_screen_pos) {
        Ok(world_pos) => world_pos.trunc(),
        Err(_) => return,
    };

    let mut dir = cursor_world - player_pos;
    if dir.length_squared() <= std::f32::EPSILON {
        return;
    }
    dir = dir.normalize();

    let ts = time_in_seconds();
    // смещаем точку спавна от центра игрока на радиус гранаты (+1 px запас)
    let spawn_from = player_pos + dir * (GRENADE_RADIUS + 1.0);

    let ev = GrenadeEvent {
        id: my.id ^ (ts as u64),
        from: spawn_from,
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
        grenade_cd.0.reset();
        info!(
            "💣 Sent ThrowGrenade {}, speed: {}, timer: {}",
            ev.id, ev.speed, ev.timer
        );
    }
}
