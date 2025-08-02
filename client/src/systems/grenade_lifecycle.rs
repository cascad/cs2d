use std::f32::consts::PI;

use crate::{
    components::{Explosion, Grenade},
    ui::components::ExplosionMaterial,
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
};

/// Движение гранаты и её взрыв
pub fn grenade_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Grenade)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let dt = time.delta_secs();
    for (ent, mut tf, mut gr) in q.iter_mut() {
        // движение
        tf.translation += (gr.dir * gr.speed * dt).extend(0.0);

        // таймер
        gr.timer.tick(time.delta());
        if gr.timer.just_finished() {
            // удаляем гранату
            commands.entity(ent).despawn();

            // todo
            // Если позже почините фичи и захотите красивую текстуру/анимацию — просто добавьте bevy_asset, подмените пункт 3 (спрайт) на SpriteBundle + texture: my_handle.clone().

            // создаём круглый взрыв
            let mesh = meshes.add(generate_circle_mesh(gr.blast_radius, 32));
            let material = materials.add(Color::srgba(1.0, 0.6, 0.2, 0.8));
            let mat_handle = material.clone();

            let mut entity = commands.spawn_empty();

            entity
                .insert(Mesh2d(mesh))
                .insert(MeshMaterial2d(material))
                .insert(Transform {
                    translation: tf.translation,
                    scale: Vec3::ONE,
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
}

pub fn explosion_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Explosion, &ExplosionMaterial)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (ent, mut exp, mat) in q.iter_mut() {
        exp.timer.tick(time.delta());

        let t = exp.timer.elapsed_secs() / exp.timer.duration().as_secs_f32();

        // println!("💥 t = {t:.2}, alpha = {}", 1.0 - t);

        if let Some(material) = materials.get_mut(&mat.0) {
            material.color.set_alpha(1.0 - t);
        }

        // sprite.color.set_alpha(1.0 - t);

        if exp.timer.finished() {
            commands.entity(ent).despawn();
        }
    }
}

// fn generate_circle_mesh(radius: f32, resolution: usize) -> Mesh {
//     let mut positions = vec![[0.0, 0.0, 0.0]];
//     let mut indices = vec![];

//     for i in 0..=resolution {
//         let angle = i as f32 / resolution as f32 * std::f32::consts::TAU;
//         positions.push([radius * angle.cos(), radius * angle.sin(), 0.0]);
//     }

//     for i in 1..resolution {
//         indices.push(0);
//         indices.push(i as u32);
//         indices.push((i + 1) as u32);
//     }

//     let mut mesh = Mesh::new(
//         PrimitiveTopology::TriangleList,
//         RenderAssetUsages::RENDER_WORLD,
//     );
//     mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
//     mesh.insert_indices(Indices::U32(indices));
//     mesh
// }

pub fn generate_circle_mesh(radius: f32, segments: usize) -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]]; // центр круга
    let mut uvs = vec![[0.5, 0.5]];
    let mut indices = vec![];

    for i in 0..=segments {
        let theta = (i as f32 / segments as f32) * PI * 2.0;
        let x = radius * theta.cos();
        let y = radius * theta.sin();
        positions.push([x, y, 0.0]);
        uvs.push([(x / (2.0 * radius)) + 0.5, (y / (2.0 * radius)) + 0.5]);
    }

    // генерируем треугольники по кругу
    for i in 1..=segments {
        indices.push(0);
        indices.push(i as u32);
        indices.push((i + 1) as u32);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
