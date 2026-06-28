// ------------------------------------------------------------------------------------------------
// client/src/systems/grenade_lifecycle.rs — гранаты с точной коллизией и отскоком по тайлам
// Bevy 0.16.1
// ------------------------------------------------------------------------------------------------
use bevy::{
    asset::RenderAssetUsages,
    math::Affine2,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    sprite_render::AlphaMode2d,
};

use crate::{
    components::{Explosion, Grenade, GrenadeNet},
    events::GrenadeSpawnEvent,
    render::{layers, world_to_translation, RenderLayer, WorldPos},
    systems::melee::make_iso_circle_mesh,
    ui::components::ExplosionMaterial,
};
use protocol::constants::GRENADE_BLAST_RADIUS;

/// На сколько экранных пикселей «зелье» висит над своей наземной тенью —
/// дешёвый псевдо-3D: тень лежит на полу (в точке `WorldPos`), сам пузырёк парит.
const POTION_LIFT: f32 = 12.0;

/// Плоский круглый диск в ЭКРАННЫХ координатах (не изо): круглый «пузырёк»/блик.
pub fn make_screen_disc_mesh(radius: f32, segments: usize) -> Mesh {
    use std::f32::consts::TAU;
    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    for i in 0..=segments {
        let a = TAU * (i as f32 / segments as f32);
        positions.push([a.cos() * radius, a.sin() * radius, 0.0]);
    }
    let mut indices: Vec<u32> = Vec::with_capacity(segments * 3);
    for i in 0..segments as u32 {
        indices.extend([0u32, 1 + i, 2 + i]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[inline]
fn flat_material(materials: &mut Assets<ColorMaterial>, color: Color) -> Handle<ColorMaterial> {
    materials.add(ColorMaterial {
        color,
        alpha_mode: AlphaMode2d::Blend.into(),
        uv_transform: Affine2::IDENTITY,
        texture: None,
    })
}

// ------------------------------------------------------------------------------------------------
// Спавн гранаты по событию от сервера (без локальной физики)
// ------------------------------------------------------------------------------------------------
//
// Визуал «зелья»: наземная тень-эллипс (изо) — это родитель с `WorldPos`, его и
// двигает сеть; над ней парят дочерние диски (стекло пузырька + магический блик).
// Так пузырёк читается в изометрии, а тень привязывает его к полу.
pub fn spawn_grenades(
    mut commands: Commands,
    mut evr: MessageReader<GrenadeSpawnEvent>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for GrenadeSpawnEvent(ev) in evr.read() {
        // Тень на полу (изо-эллипс), родитель сущности.
        let shadow_mesh = meshes.add(make_iso_circle_mesh(8.0, 20));
        let shadow_mat = flat_material(&mut materials, Color::srgba(0.0, 0.0, 0.0, 0.30));

        // Дочерние диски: внешнее свечение + ядро-«стекло».
        let glow_mesh = meshes.add(make_screen_disc_mesh(11.0, 20));
        let glow_mat = flat_material(&mut materials, Color::srgba(0.55, 0.40, 1.0, 0.30));
        let body_mesh = meshes.add(make_screen_disc_mesh(6.5, 20));
        let body_mat = flat_material(&mut materials, Color::srgba(0.78, 0.62, 1.0, 0.95));

        commands
            .spawn((
                Mesh2d(shadow_mesh),
                MeshMaterial2d(shadow_mat),
                Transform::from_translation(world_to_translation(ev.from, layers::EFFECT)),
                WorldPos(ev.from),
                RenderLayer(layers::EFFECT),
                GlobalTransform::default(),
                Visibility::Visible,
                InheritedVisibility::default(),
                ViewVisibility::default(),
                Grenade {
                    id: ev.id,
                    from: ev.from,
                    dir: ev.dir,
                    speed: ev.speed,
                    timer: Timer::from_seconds(ev.timer, TimerMode::Once),
                    blast_radius: GRENADE_BLAST_RADIUS,
                },
                GrenadeNet { id: ev.id },
            ))
            .with_children(|p| {
                // свечение (ниже ядра по Z)
                p.spawn((
                    Mesh2d(glow_mesh),
                    MeshMaterial2d(glow_mat),
                    Transform::from_xyz(0.0, POTION_LIFT, 0.05),
                ));
                // ядро пузырька (выше свечения)
                p.spawn((
                    Mesh2d(body_mesh),
                    MeshMaterial2d(body_mat),
                    Transform::from_xyz(0.0, POTION_LIFT, 0.10),
                ));
            });

        info!("🧪 Spawned client potion id={} at {:?}", ev.id, ev.from);
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
        if exp.timer.is_finished() {
            commands.entity(ent).despawn();
        }
    }
}

