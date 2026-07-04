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
    components::Explosion,
    ui::components::ExplosionMaterial,
};

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

