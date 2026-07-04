use crate::{
    components::LocalPlayer,
    render::{depth_z, layers, world_to_screen, WorldPos},
    systems::melee::make_iso_ring_mesh,
};
use bevy::prelude::*;
use protocol::constants::grenade_max_reach;

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
