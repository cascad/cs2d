use crate::ui::components::ExplosionMaterial;
use crate::{
    components::{Explosion, Grenade},
    events::GrenadeDetonatedEvent,
    systems::grenade_lifecycle::generate_circle_mesh,
};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::sprite::AlphaMode2d;
use protocol::constants::GRENADE_BLAST_RADIUS;

// ------------------------------------------------------------------------------------------------
// Рендер детонаций по серверному событию
// ------------------------------------------------------------------------------------------------
pub fn render_detonations(
    mut commands: Commands,
    mut evr: EventReader<GrenadeDetonatedEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    q_gren: Query<(Entity, &Grenade)>,
) {
    for e in evr.read() {
        // удалить визуальную гранату (если ещё не удалена)
        if let Some((ent, _)) = q_gren.iter().find(|(_, g)| g.id == e.id) {
            commands.entity(ent).despawn();
            info!("🎇 Detonated client grenade id={} at {:?}", e.id, e.pos);
        }

        // FX строго в pos от сервера
        let mesh = meshes.add(generate_circle_mesh(GRENADE_BLAST_RADIUS, 32));
        let material = materials.add(ColorMaterial {
            color: Color::srgba(1.0, 0.6, 0.2, 0.8),
            alpha_mode: AlphaMode2d::Blend.into(),
            uv_transform: Affine2::IDENTITY,
            texture: None,
        });
        let mat_handle = material.clone();

        commands
            .spawn_empty()
            .insert(Mesh2d(mesh))
            .insert(MeshMaterial2d(material))
            .insert(Transform {
                translation: e.pos.extend(1.0),
                ..default()
            })
            .insert(GlobalTransform::default())
            .insert(Visibility::Visible)
            .insert(InheritedVisibility::default())
            .insert(ViewVisibility::default())
            .insert(Explosion {
                timer: Timer::from_seconds(0.4, TimerMode::Once),
            })
            .insert(ExplosionMaterial(mat_handle));
    }
}
