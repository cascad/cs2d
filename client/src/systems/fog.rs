//! Клиентская часть тумана войны.
//!
//! Безопасность обеспечивается СЕРВЕРОМ (он не присылает невидимых игроков —
//! см. `server_tick`). Здесь только две вещи поверх этого:
//!   1. `fade_unseen_players` — плавно гасит и деспавнит тех, кого сервер
//!      перестал присылать (вышли из зоны видимости);
//!   2. `setup_fog`/`update_fog` — «диабловское» затемнение карты: тёмный меш,
//!      в котором вырезан полигон видимости (360° рейкасты по стенам в радиусе
//!      VIEW_RADIUS). Радиус совпадает с серверным, чтобы ясная зона на клиенте
//!      не выходила за пределы того, что реально прислано.

use bevy::asset::RenderAssetUsages;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::sprite::AlphaMode2d;
use protocol::constants::VIEW_RADIUS;
use protocol::geom::WallGrid;

use crate::components::{LocalPlayer, PlayerMarker};
use crate::render::layers;
use crate::resources::{LastSeen, MyPlayer, SpawnedPlayers, WallGridRes};
use crate::systems::utils::time_in_seconds;

// --- Плавное скрытие пропавших игроков ---
const FADE_START: f64 = 0.20; // до этого возраста (сек без снапшота) — видно полностью
const FADE_END: f64 = 0.60; // после — деспавн

/// Гасит альфу игроков (вместе с дочерним «стволом») по времени с последнего
/// появления в снапшоте; полностью пропавших деспавнит и снимает с учёта, чтобы
/// при повторном появлении они заспавнились заново.
pub fn fade_unseen_players(
    mut commands: Commands,
    my: Res<MyPlayer>,
    mut last_seen: ResMut<LastSeen>,
    mut spawned: ResMut<SpawnedPlayers>,
    mut sets: ParamSet<(
        Query<(Entity, &PlayerMarker, Option<&Children>)>,
        Query<&mut Sprite>,
    )>,
) {
    let now = time_in_seconds();

    let mut to_set: Vec<(Entity, f32)> = Vec::new();
    let mut to_despawn: Vec<(Entity, u64)> = Vec::new();
    {
        let q = sets.p0();
        for (e, marker, children) in q.iter() {
            if marker.0 == my.id {
                continue; // себя не трогаем
            }
            let age = last_seen.0.get(&marker.0).map(|&t| now - t).unwrap_or(0.0);
            if age >= FADE_END {
                to_despawn.push((e, marker.0));
                continue;
            }
            let alpha = if age <= FADE_START {
                1.0
            } else {
                (((FADE_END - age) / (FADE_END - FADE_START)) as f32).clamp(0.0, 1.0)
            };
            to_set.push((e, alpha));
            if let Some(ch) = children {
                for c in ch.iter() {
                    to_set.push((c, alpha));
                }
            }
        }
    }
    {
        let mut q = sets.p1();
        for (e, a) in to_set {
            if let Ok(mut s) = q.get_mut(e) {
                s.color.set_alpha(a);
            }
        }
    }
    for (e, id) in to_despawn {
        commands.entity(e).despawn();
        spawned.0.remove(&id);
        last_seen.0.remove(&id);
    }
}

// --- Затемнение карты (полигон видимости) ---
const FOG_SEGMENTS: usize = 144;
const FOG_OUTER: f32 = 8000.0; // «бесконечность» — заведомо за пределами экрана
const FOG_ALPHA: f32 = 0.92;

#[derive(Component)]
pub struct FogOverlay;

pub fn setup_fog(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mesh = meshes.add(empty_mesh());
    let material = materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, FOG_ALPHA),
        alpha_mode: AlphaMode2d::Blend.into(),
        uv_transform: Affine2::IDENTITY,
        texture: None,
    });
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, layers::FOG),
        GlobalTransform::default(),
        Visibility::Visible,
        InheritedVisibility::default(),
        ViewVisibility::default(),
        FogOverlay,
    ));
}

/// Каждый кадр перестраиваем тёмный меш с «дыркой» видимости вокруг игрока.
pub fn update_fog(
    player_q: Query<&crate::render::WorldPos, With<LocalPlayer>>,
    walls: Res<WallGridRes>,
    fog_q: Query<(&Mesh2d, &mut Transform), With<FogOverlay>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(wp) = player_q.single() else { return };
    let mut fog_q = fog_q;
    let Ok((mesh2d, mut tf)) = fog_q.single_mut() else { return };
    let center = wp.0;
    tf.translation.x = center.x;
    tf.translation.y = center.y;
    tf.translation.z = layers::FOG;
    if let Some(mesh) = meshes.get_mut(&mesh2d.0) {
        rebuild_fog_mesh(mesh, center, &walls.0);
    }
}

fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

/// Тёмная зона = ВСЁ за пределами полигона видимости. Для каждого углового
/// сектора строим четырёхугольник от границы видимости (рейкаст по стенам,
/// ограниченный VIEW_RADIUS) до «бесконечности». Координаты — локальные
/// относительно игрока (translation энтити = позиция игрока).
fn rebuild_fog_mesh(mesh: &mut Mesh, center: Vec2, walls: &WallGrid) {
    let n = FOG_SEGMENTS;
    let mut inner: Vec<Vec2> = Vec::with_capacity(n + 1);
    let mut outer: Vec<Vec2> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = (i as f32 / n as f32) * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        let d = walls.raycast(center, dir, VIEW_RADIUS).min(VIEW_RADIUS);
        inner.push(dir * d);
        outer.push(dir * FOG_OUTER);
    }

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6);
    for i in 0..n {
        let base = positions.len() as u32;
        positions.push([inner[i].x, inner[i].y, 0.0]);
        positions.push([inner[i + 1].x, inner[i + 1].y, 0.0]);
        positions.push([outer[i + 1].x, outer[i + 1].y, 0.0]);
        positions.push([outer[i].x, outer[i].y, 0.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let vcount = positions.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; vcount]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; vcount]);
    mesh.insert_indices(Indices::U32(indices));
}
