// ------------------------------------------------------------------------------------------------
// client/src/systems/grenade_lifecycle.rs — гранаты с точной коллизией и отскоком по тайлам
// Bevy 0.16.1
// ------------------------------------------------------------------------------------------------
use bevy::{
    asset::RenderAssetUsages,
    math::Affine2,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
    sprite::AlphaMode2d,
};

use crate::{
    components::{Explosion, Grenade, GrenadeNet},
    events::GrenadeSpawnEvent,
    render::{layers, world_to_translation, RenderLayer, WorldPos},
    ui::components::ExplosionMaterial,
};
use protocol::constants::GRENADE_BLAST_RADIUS;

// ------------------------------------------------------------------------------------------------
// Спавн гранаты по событию от сервера (без локальной физики)
// ------------------------------------------------------------------------------------------------
pub fn spawn_grenades(
    mut commands: Commands,
    mut evr: EventReader<GrenadeSpawnEvent>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for GrenadeSpawnEvent(ev) in evr.read() {
        // Плоский квадрат 16×16 (визуал)
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-8.0, -8.0, 0.0],
                [8.0, -8.0, 0.0],
                [8.0, 8.0, 0.0],
                [-8.0, 8.0, 0.0],
            ],
        );
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        let mesh = meshes.add(mesh);

        let material = materials.add(ColorMaterial {
            color: Color::srgb(0.9, 0.15, 0.15),
            alpha_mode: AlphaMode2d::Blend.into(),
            uv_transform: Affine2::IDENTITY,
            texture: None,
        });

        commands
            .spawn_empty()
            .insert(Mesh2d(mesh))
            .insert(MeshMaterial2d(material))
            .insert(Transform::from_translation(world_to_translation(
                ev.from,
                layers::EFFECT,
            )))
            .insert(WorldPos(ev.from))
            .insert(RenderLayer(layers::EFFECT))
            .insert(GlobalTransform::default())
            .insert(Visibility::Visible)
            .insert(InheritedVisibility::default())
            .insert(ViewVisibility::default())
            // Компоненты игры (таймер можно оставить только для UI/эффектов, физика больше не использует)
            .insert(Grenade {
                id: ev.id,
                from: ev.from,
                dir: ev.dir,     // не используется локальной физикой
                speed: ev.speed, // не используется локальной физикой
                timer: Timer::from_seconds(ev.timer, TimerMode::Once),
                blast_radius: GRENADE_BLAST_RADIUS,
            })
            // сетевой id для привязки к снапшотам
            .insert(GrenadeNet { id: ev.id });

        info!("🧨 Spawned client grenade id={} at {:?}", ev.id, ev.from);
    }
}

// ------------------------------------------------------------------------------------------------
// Затухание взрыва
// ------------------------------------------------------------------------------------------------
pub fn explosion_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Explosion, &ExplosionMaterial)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (ent, mut exp, mat) in q.iter_mut() {
        exp.timer.tick(time.delta());
        let t = exp.timer.elapsed_secs() / exp.timer.duration().as_secs_f32();
        if let Some(material) = materials.get_mut(&mat.0) {
            material.color.set_alpha(1.0 - t);
        }
        if exp.timer.finished() {
            commands.entity(ent).despawn();
        }
    }
}

