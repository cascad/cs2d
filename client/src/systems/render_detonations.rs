use crate::systems::level::Wall;
use crate::systems::utils::raycast_to_walls;
use crate::ui::components::ExplosionMaterial;
use crate::{
    components::{Explosion, Grenade},
    events::GrenadeDetonatedEvent,
};
use bevy::asset::RenderAssetUsages;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
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
    wall_q: Query<(&Transform, &Sprite), With<Wall>>, // ← добавили
) {
    for e in evr.read() {
        if let Some((ent, _)) = q_gren.iter().find(|(_, g)| g.id == e.id) {
            commands.entity(ent).despawn();
        }

        // генерим меш с «обрезкой» по стенам
        let mesh = meshes.add(generate_occluded_explosion_mesh(
            e.pos,
            GRENADE_BLAST_RADIUS,
            96, // сегментов хватит
            &wall_q,
        ));

        let material = materials.add(ColorMaterial {
            color: Color::srgba(1.0, 0.6, 0.2, 0.85),
            alpha_mode: AlphaMode2d::Blend.into(),
            uv_transform: Affine2::IDENTITY,
            texture: None,
        });
        let mat_handle = material.clone();

        commands.spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform {
                translation: e.pos.extend(1.0),
                ..default()
            },
            GlobalTransform::default(),
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
            Explosion {
                timer: Timer::from_seconds(0.4, TimerMode::Once),
            },
            ExplosionMaterial(mat_handle),
        ));
    }
}



// ---------- Генерация «обрезанного» меша взрыва (треугольный фан) ----------
fn generate_occluded_explosion_mesh(
    center: Vec2,
    radius: f32,
    segments: usize,
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,
) -> Mesh {
    let mut positions = Vec::with_capacity(1 + segments + 1);
    let mut uvs = Vec::with_capacity(1 + segments + 1);
    let mut indices = Vec::with_capacity(segments * 3);

    // центр
    positions.push([0.0, 0.0, 0.0]);
    uvs.push([0.5, 0.5]);

    // точки по окружности/стенам
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let theta = t * std::f32::consts::TAU;
        let dir = Vec2::new(theta.cos(), theta.sin());

        let d = raycast_to_walls(center, dir, radius, wall_q); // обрезка стенами
        let x = dir.x * d;
        let y = dir.y * d;

        positions.push([x, y, 0.0]);
        // простые UV под круг (нормально для градиента/заливки)
        uvs.push([(x / (2.0 * radius)) + 0.5, (y / (2.0 * radius)) + 0.5]);
    }

    // треугольники «веера»
    for i in 1..=segments {
        indices.extend([0u32, i as u32, (i + 1) as u32]);
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
