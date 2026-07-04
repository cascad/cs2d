use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use protocol::combat::in_melee_sector;
use protocol::constants::{MELEE_HALF_ANGLE, MELEE_HALF_WIDTH, MELEE_RANGE};

use crate::components::{Facing, LocalPlayer};
use crate::render::{depth_z, world_to_screen, WorldPos};

/// Слой декали удара: над полом/стенами (видно на земле), но ПОД актёром (рыцарь
/// стоит поверх неё). Между CORPSE(150) и ACTOR(200).
pub const MELEE_DECAL_LAYER: f32 = 170.0;
/// Время жизни/затухания зоны удара на полу = задержке нанесения урона: декаль
/// вспыхивает на замахе и полностью гаснет РОВНО в момент, когда сервер резолвит
/// попадание (середина клипа взмаха). Визуальный «прогрев» зоны = реальный тайминг.
pub const MELEE_ARC_TTL: f32 = protocol::constants::MELEE_HIT_DELAY as f32;

/// Постоянная (всегда видимая) подсветка зоны удара — тонкий ЯРКИЙ контур, чтобы
/// игрок видел, куда «дотянется» взмах, но заливка не «забивала» пол. Цвет —
/// холодный голубой: контрастирует с тёплым (желтоватым) полом подземелья.
/// Во время самого удара поверх него вспыхивает жёлтая декаль-заливка
/// (`spawn_player_melee_decal`).
const MELEE_HINT_ALPHA: f32 = 0.85;
const MELEE_HINT_THICKNESS: f32 = 3.0;
const MELEE_HINT_COLOR: Color = Color::srgba(0.25, 0.95, 1.0, MELEE_HINT_ALPHA);

/// Вспышка в момент удара — ЗАЛИВКА того же сектора тем же голубым цветом (раньше
/// была жёлтой). Стартовая прозрачность; гаснет за [`MELEE_ARC_TTL`].
const MELEE_FLASH_ALPHA: f32 = 0.40;

/// Декаль зоны урона ближнего боя на земле — гаснет за [`MELEE_ARC_TTL`].
#[derive(Component)]
pub struct MeleeArc {
    pub timer: Timer,
}

/// Спавнит голубую декаль-заливку зоны удара игрока — 1:1 с серверным
/// `in_melee_sector` (тот же сектор, что обведён постоянным контуром).
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
    let mat = materials.add(ColorMaterial::from(Color::srgba(0.25, 0.95, 1.0, MELEE_FLASH_ALPHA)));
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
            m.color.set_alpha(MELEE_FLASH_ALPHA * frac);
        }
        if arc.timer.is_finished() {
            commands.entity(e).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Постоянная подсветка зоны удара (следует за направлением модели)
// ---------------------------------------------------------------------------

/// Маркер сущности-подсветки зоны удара; хранит угол, по которому построен меш,
/// чтобы не пересобирать его каждый кадр без нужды.
#[derive(Component)]
pub struct MeleeHint {
    last_angle: f32,
}

/// Спавн постоянной (еле заметной) подсветки зоны удара. Один раз при входе в
/// игру. Меш пересобирается в `update_melee_hint` под текущее направление.
pub fn setup_melee_hint(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mesh = meshes.add(make_melee_outline_mesh(0.0, MELEE_HINT_THICKNESS));
    let mat = materials.add(ColorMaterial::from(MELEE_HINT_COLOR));
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        // ПОД декалью удара и под актёром: чуть ниже MELEE_DECAL_LAYER.
        Transform::from_xyz(0.0, 0.0, MELEE_DECAL_LAYER - 1.0),
        Visibility::Hidden, // покажем, когда найдём игрока
        MeleeHint { last_angle: f32::NAN },
    ));
}

/// Каждый кадр держим подсветку под ногами игрока и поворачиваем по направлению
/// модели (`Facing`) — куда реально уйдёт взмах. Меш перестраиваем только при
/// заметном изменении угла (проекция изо нелинейна по повороту, простой rotate
/// не годится).
pub fn update_melee_hint(
    player_q: Query<(&WorldPos, &Facing), With<LocalPlayer>>,
    mut hint_q: Query<
        (&mut Transform, &mut Visibility, &mut MeleeHint, &Mesh2d),
        Without<LocalPlayer>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok((wp, facing)) = player_q.single() else {
        // нет игрока — прячем подсветку
        if let Ok((_, mut vis, _, _)) = hint_q.single_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let Ok((mut tf, mut vis, mut hint, mesh)) = hint_q.single_mut() else { return };

    *vis = Visibility::Visible;
    let s = world_to_screen(wp.0);
    tf.translation.x = s.x;
    tf.translation.y = s.y;
    tf.translation.z = depth_z(wp.0, MELEE_DECAL_LAYER - 1.0);

    // перестраиваем меш только при заметном повороте (> ~1.7°)
    if !hint.last_angle.is_finite() || (facing.0 - hint.last_angle).abs() > 0.03 {
        if let Some(m) = meshes.get_mut(&mesh.0) {
            *m = make_melee_outline_mesh(facing.0, MELEE_HINT_THICKNESS);
        }
        hint.last_angle = facing.0;
    }
}

/// Контур зоны удара игрока в МИРОВЫХ координатах (замкнутый полигон), построенный
/// по `in_melee_sector` (тот же тест, что на сервере) — «что обведено = что бьёт».
fn melee_hull_world(dir_angle: f32) -> Vec<Vec2> {
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

    hull
}

/// Меш-ЗАЛИВКА зоны удара (треугольный веер из центра) — для яркой вспышки удара.
pub fn make_iso_melee_hit_mesh(dir_angle: f32) -> Mesh {
    let hull = melee_hull_world(dir_angle);

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

/// Меш-КОНТУР зоны удара: тонкая полоса (лента) шириной `thickness` px вдоль
/// границы сектора в ЭКРАННЫХ координатах. Внутренний контур получаем сдвигом
/// каждой вершины внутрь (к центроиду) — для выпуклого сектора это даёт ровный,
/// хорошо заметный ободок, который не «забивает» пол заливкой.
pub fn make_melee_outline_mesh(dir_angle: f32, thickness: f32) -> Mesh {
    let hull_world = melee_hull_world(dir_angle);

    // проецируем контур в экран
    let outer: Vec<Vec2> = hull_world.iter().map(|w| world_to_screen(*w)).collect();
    let n = outer.len();

    // центроид (в экранных координатах) — направление «внутрь» для смещения
    let centroid = outer.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / n as f32;

    let inner: Vec<Vec2> = outer
        .iter()
        .map(|p| {
            let to_c = (centroid - *p).normalize_or_zero();
            *p + to_c * thickness
        })
        .collect();

    // лента: на каждый сегмент — два треугольника между outer[i]/inner[i] и
    // outer[i+1]/inner[i+1] (контур замкнут).
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 2);
    for i in 0..n {
        positions.push([outer[i].x, outer[i].y, 0.0]);
        positions.push([inner[i].x, inner[i].y, 0.0]);
    }
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6);
    for i in 0..n as u32 {
        let o0 = 2 * i;
        let i0 = 2 * i + 1;
        let o1 = 2 * ((i + 1) % n as u32);
        let i1 = 2 * ((i + 1) % n as u32) + 1;
        indices.extend([o0, i0, o1, o1, i0, i1]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Кольцо-КОНТУР (изо-эллипс) шириной `thickness` мир. ед. — для подсветки зоны
/// (например, «куда долетит граната»). Толщина задаётся в мире, поэтому ободок
/// ложится на пол ровной полосой.
pub fn make_iso_ring_mesh(radius: f32, thickness: f32, segments: usize) -> Mesh {
    use std::f32::consts::TAU;
    let inner_r = (radius - thickness).max(0.0);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity((segments + 1) * 2);
    for i in 0..=segments {
        let a = TAU * (i as f32 / segments as f32);
        let d = Vec2::new(a.cos(), a.sin());
        let o = world_to_screen(d * radius);
        let inr = world_to_screen(d * inner_r);
        positions.push([o.x, o.y, 0.0]);
        positions.push([inr.x, inr.y, 0.0]);
    }
    let mut indices: Vec<u32> = Vec::with_capacity(segments * 6);
    for i in 0..segments as u32 {
        let o0 = 2 * i;
        let i0 = 2 * i + 1;
        let o1 = 2 * i + 2;
        let i1 = 2 * i + 3;
        indices.extend([o0, i0, o1, o1, i0, i1]);
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
