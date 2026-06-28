use crate::{
    components::LocalPlayer,
    render::{depth_z, layers, pointer_world, world_to_screen, WorldPos},
    resources::{MyPlayer, grenades::GrenadeCooldown},
    systems::{melee::make_iso_ring_mesh, utils::time_in_seconds},
};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::{
    constants::{grenade_max_reach, CH_C2S, GRENADE_RADIUS, GRENADE_SPEED, GRENADE_TIMER},
    messages::{GrenadeEvent, C2S},
};

/// Голубой контур-подсказка дальности броска: кольцо ВОКРУГ игрока радиусом с
/// максимальную дальность (`grenade_max_reach`) — оно и есть предел. Показывается
/// только пока зажата G; бросок — по отпусканию.
const GRENADE_AIM_THICKNESS: f32 = 6.0;
const GRENADE_AIM_COLOR: Color = Color::srgba(0.25, 0.95, 1.0, 0.5);

#[derive(Component)]
pub struct GrenadeAim;

/// Один раз спавним кольцо-подсказку (скрыто, пока нет игрока/курсора).
pub fn setup_grenade_aim(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Радиус постоянен (`grenade_max_reach`), поэтому меш строим единожды.
    let mesh = meshes.add(make_iso_ring_mesh(grenade_max_reach(), GRENADE_AIM_THICKNESS, 64));
    let mat = materials.add(ColorMaterial::from(GRENADE_AIM_COLOR));
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, layers::AIM),
        Visibility::Hidden,
        GrenadeAim,
    ));
}

/// Пока зажата G — держим кольцо-предел дальности вокруг игрока; иначе прячем.
pub fn update_grenade_aim(
    keys: Res<ButtonInput<KeyCode>>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
    mut aim_q: Query<(&mut Transform, &mut Visibility), With<GrenadeAim>>,
) {
    let Ok((mut tf, mut vis)) = aim_q.single_mut() else { return };

    if !keys.pressed(KeyCode::KeyG) {
        *vis = Visibility::Hidden;
        return;
    }
    let Ok(player) = player_q.single() else {
        *vis = Visibility::Hidden;
        return;
    };

    let s = world_to_screen(player.0);
    tf.translation.x = s.x;
    tf.translation.y = s.y;
    tf.translation.z = depth_z(player.0, layers::AIM);
    *vis = Visibility::Visible;
}

pub fn grenade_throw(
    keys: Res<ButtonInput<KeyCode>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    player_query: Query<&WorldPos, With<LocalPlayer>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    mut grenade_cd: ResMut<GrenadeCooldown>,
    time: Res<Time>,
) {
    grenade_cd.0.tick(time.delta());

    // Бросок происходит по ОТПУСКАНИЮ G (пока зажата — показываем предел дальности).
    if !keys.just_released(KeyCode::KeyG) || !grenade_cd.0.is_finished() {
        return;
    }

    let player_pos = match player_query.single() {
        Ok(wp) => wp.0,
        Err(_) => return,
    };

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };

    let (camera, cam_transform) = match camera_q.single() {
        Ok(c) => c,
        Err(_) => return,
    };

    let cursor_world = match pointer_world(window, camera, cam_transform) {
        Some(world_pos) => world_pos.trunc(),
        None => return,
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
        // Куда указал курсор — точка падения. Сервер сам сделает кламп до
        // максимальной дальности и подорвёт при достижении.
        target: cursor_world,
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
