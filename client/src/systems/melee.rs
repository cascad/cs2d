use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use protocol::combat::in_melee_sector;
use protocol::constants::{MELEE_HALF_ANGLE, MELEE_HALF_WIDTH, MELEE_RANGE};

use crate::render::{depth_z, world_to_screen};

/// Слой декали удара: над полом/стенами (видно на земле), но ПОД актёром (рыцарь
/// стоит поверх неё). Между CORPSE(150) и ACTOR(200).
pub const MELEE_DECAL_LAYER: f32 = 170.0;
/// Время жизни/затухания зоны удара на полу.
pub const MELEE_ARC_TTL: f32 = 0.22;

/// Декаль зоны урона ближнего боя на земле — гаснет за [`MELEE_ARC_TTL`].
#[derive(Component)]
pub struct MeleeArc {
    pub timer: Timer,
}

/// Спавнит жёлтую декаль зоны удара игрока — 1:1 с серверным `in_melee_sector`.
pub fn spawn_player_melee_decal(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    player_pos: Vec2,
    dir: Vec2,
) {
    if dir == Vec2::ZERO {
        return;
    }
    let dir_angle = dir.y.atan2(dir.x);
    let mesh = make_iso_melee_hit_mesh(dir_angle);
    let mat = materials.add(ColorMaterial::from(Color::srgba(1.0, 0.82, 0.25, 0.32)));
    let s = world_to_screen(player_pos);
    commands.spawn((
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(mat),
        Transform::from_xyz(s.x, s.y, depth_z(player_pos, MELEE_DECAL_LAYER)),
        MeleeArc {
            timer: Timer::from_seconds(MELEE_ARC_TTL, TimerMode::Once),
        },
    ));
}

/// Затухание и удаление декали удара.
pub fn melee_arc_lifecycle(
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(Entity, &mut MeleeArc, &MeshMaterial2d<ColorMaterial>)>,
    mut commands: Commands,
) {
    for (e, mut arc, mat) in q.iter_mut() {
        arc.timer.tick(time.delta());
        let frac = 1.0 - arc.timer.fraction();
        if let Some(m) = materials.get_mut(&mat.0) {
            m.color.set_alpha(0.32 * frac);
        }
        if arc.timer.is_finished() {
            commands.entity(e).despawn();
        }
    }
}

/// Меш зоны удара игрока: контур строим по `in_melee_sector` (тот же тест, что на
/// сервере) — «что видишь на полу = что бьёт».
fn make_iso_melee_hit_mesh(dir_angle: f32) -> Mesh {
    use std::f32::consts::{FRAC_PI_2, PI};

    let dir = Vec2::new(dir_angle.cos(), dir_angle.sin());
    let left_perp = Vec2::new(-dir.y, dir.x);
    let right_perp = -left_perp;

    let hit = |p: Vec2| {
        in_melee_sector(
            Vec2::ZERO,
            dir,
            p,
            0.0,
            MELEE_RANGE,
            MELEE_HALF_ANGLE,
            MELEE_HALF_WIDTH,
        )
    };

    let mut hull: Vec<Vec2> = Vec::new();

    // Передняя полуокружность у ног (постоянная ширина вблизи).
    const NEAR_SEG: usize = 10;
    for i in 0..=NEAR_SEG {
        let a = dir_angle + FRAC_PI_2 - PI * (i as f32 / NEAR_SEG as f32);
        hull.push(Vec2::new(a.cos(), a.sin()) * MELEE_HALF_WIDTH);
    }

    // Правый бок капсулы.
    const SIDE_SEG: usize = 4;
    for i in 1..=SIDE_SEG {
        let t = i as f32 / SIDE_SEG as f32;
        let p = dir * (MELEE_RANGE * t) + right_perp * MELEE_HALF_WIDTH;
        if hit(p) {
            hull.push(p);
        }
    }

    // Дальняя дуга: по лучам ±MELEE_HALF_ANGLE — макс. дистанция, где ещё попадает.
    const FAR_SEG: usize = 12;
    for i in 0..=FAR_SEG {
        let a = dir_angle - MELEE_HALF_ANGLE
            + 2.0 * MELEE_HALF_ANGLE * (i as f32 / FAR_SEG as f32);
        let ray = Vec2::new(a.cos(), a.sin());
        let mut lo = 0.0f32;
        let mut hi = MELEE_RANGE + MELEE_HALF_WIDTH;
        for _ in 0..14 {
            let mid = (lo + hi) * 0.5;
            if hit(ray * mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hull.push(ray * lo);
    }

    // Левый бок капсулы (обратно к ногам).
    for i in (1..=SIDE_SEG).rev() {
        let t = i as f32 / SIDE_SEG as f32;
        let p = dir * (MELEE_RANGE * t) + left_perp * MELEE_HALF_WIDTH;
        if hit(p) {
            hull.push(p);
        }
    }

    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    for w in &hull {
        let s = world_to_screen(*w);
        positions.push([s.x, s.y, 0.0]);
    }
    let n = hull.len() as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(n as usize * 3);
    for i in 0..n {
        indices.extend([0u32, 1 + i, 1 + (i + 1) % n]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Круговая зона (для удара неписи: сервер бьёт по радиусу `NPC_ATTACK_RANGE`).
pub fn make_iso_circle_mesh(radius: f32, segments: usize) -> Mesh {
    use std::f32::consts::TAU;
    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    for i in 0..=segments {
        let a = TAU * (i as f32 / segments as f32);
        let w = Vec2::new(a.cos(), a.sin()) * radius;
        let s = world_to_screen(w);
        positions.push([s.x, s.y, 0.0]);
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
