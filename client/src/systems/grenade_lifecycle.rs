// ------------------------------------------------------------------------------------------------
// client/src/systems/grenade_lifecycle.rs — гранаты с точной коллизией и отскоком по тайлам
// Bevy 0.16.1
// ------------------------------------------------------------------------------------------------
use std::f32::consts::PI;

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
    systems::level::Wall,
    ui::components::ExplosionMaterial,
};
use protocol::constants::{GRENADE_BLAST_RADIUS, SEPARATION_EPS, TILE_SIZE};

// ------------------------------------------------------------------------------------------------
// Утилиты коллизии: круг (центр c, радиус r) против тайловой стены (AABB тайла)
// ------------------------------------------------------------------------------------------------

#[inline]
fn tile_aabb(tile: IVec2) -> (Vec2, Vec2) {
    let min = Vec2::new(tile.x as f32 * TILE_SIZE, tile.y as f32 * TILE_SIZE);
    let max = min + Vec2::splat(TILE_SIZE);
    (min, max)
}

#[inline]
fn clamp2(p: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
}

fn collide_circle_with_rects(center: Vec2, r: f32, rects: &[(Vec2, Vec2)]) -> Option<(Vec2, Vec2)> {
    let sep_eps = 0.5;

    #[inline]
    fn clamp2(p: Vec2, min: Vec2, max: Vec2) -> Vec2 {
        Vec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
    }

    for &(min_b, max_b) in rects {
        let closest = clamp2(center, min_b, max_b);
        let delta = center - closest;
        let d2 = delta.length_squared();

        if d2 <= r * r {
            if d2 > 0.0 {
                let dist = d2.sqrt();
                let n = delta / dist;
                let push = (r - dist) + sep_eps;
                return Some((n, n * push));
            } else {
                // центр внутри прямоугольника — толкаем по кратчайшей оси
                let pen_left = (center.x - min_b.x).abs();
                let pen_right = (max_b.x - center.x).abs();
                let pen_bottom = (center.y - min_b.y).abs();
                let pen_top = (max_b.y - center.y).abs();
                let (n, push) = {
                    let min_x = pen_left.min(pen_right);
                    let min_y = pen_bottom.min(pen_top);
                    if min_x < min_y {
                        if pen_left < pen_right {
                            (Vec2::NEG_X, r + sep_eps)
                        } else {
                            (Vec2::X, r + sep_eps)
                        }
                    } else {
                        if pen_bottom < pen_top {
                            (Vec2::NEG_Y, r + sep_eps)
                        } else {
                            (Vec2::Y, r + sep_eps)
                        }
                    }
                };
                return Some((n, n * push));
            }
        }
    }
    None
}

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
            .insert(Transform {
                translation: ev.from.extend(1.0),
                ..Default::default()
            })
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

// ------------------------------------------------------------------------------------------------
// Генератор треугольного меша круга (для FX)
// ------------------------------------------------------------------------------------------------
pub fn generate_circle_mesh(radius: f32, segments: usize) -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]]; // центр
    let mut uvs = vec![[0.5, 0.5]];
    let mut indices = vec![];

    for i in 0..=segments {
        let theta = (i as f32 / segments as f32) * PI * 2.0;
        let x = radius * theta.cos();
        let y = radius * theta.sin();
        positions.push([x, y, 0.0]);
        uvs.push([(x / (2.0 * radius)) + 0.5, (y / (2.0 * radius)) + 0.5]);
    }

    for i in 1..=segments {
        indices.extend([0, i as u32, (i + 1) as u32]);
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

/// Точная коллизия круг (центр `c`, радиус `r`) против прямоугольника стены.
/// Возвращает (normal, correction), где:
/// - normal — наружная нормаль поверхности контакта;
/// - correction — вектор минимальной коррекции центра круга, чтобы выйти из пересечения.
fn collide_circle_with_wall_precise(
    c: Vec2,
    r: f32,
    wt: &Transform,
    sprite: &Sprite,
) -> Option<(Vec2, Vec2)> {
    let size = sprite.custom_size?;
    let half_w = size / 2.0;
    let rect_center = wt.translation.truncate();
    let min_b = rect_center - half_w;
    let max_b = rect_center + half_w;

    // Ближайшая точка прямоугольника к центру круга
    let closest = clamp_vec2(c, min_b, max_b);
    let delta = c - closest;
    let d2 = delta.length_squared();

    if d2 > r * r {
        return None; // нет пересечения
    }

    // Есть пересечение.
    if d2 > 0.0 {
        // Обычный случай: ближайшая точка на стороне/углу → нормаль = нормализованный delta
        let dist = d2.sqrt();
        let n = delta / dist;
        // сколько нужно вытолкнуть из прямоугольника
        let push = (r - dist) + SEPARATION_EPS;
        return Some((n, n * push));
    }

    // Редкий случай: центр круга уже внутри прямоугольника (closest == c).
    // Выбираем ось с минимальным проникновением до стороны и толкаем по ней.
    let pen_left = (c.x - min_b.x).abs();
    let pen_right = (max_b.x - c.x).abs();
    let pen_bottom = (c.y - min_b.y).abs();
    let pen_top = (max_b.y - c.y).abs();

    let (n, push) = {
        let min_x = pen_left.min(pen_right);
        let min_y = pen_bottom.min(pen_top);
        if min_x < min_y {
            if pen_left < pen_right {
                (Vec2::NEG_X, (r + SEPARATION_EPS))
            } else {
                (Vec2::X, (r + SEPARATION_EPS))
            }
        } else {
            if pen_bottom < pen_top {
                (Vec2::NEG_Y, (r + SEPARATION_EPS))
            } else {
                (Vec2::Y, (r + SEPARATION_EPS))
            }
        }
    };
    Some((n, n * push))
}

/// Проверка по всем стенам: возвращает первую найденную нормаль и коррекцию
fn collide_circle_with_walls(
    center: Vec2,
    r: f32,
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,
) -> Option<(Vec2, Vec2)> {
    for (wt, sprite) in wall_q.iter() {
        if let Some(hit) = collide_circle_with_wall_precise(center, r, wt, sprite) {
            return Some(hit);
        }
    }
    None
}

// clamp по компонентам
#[inline]
fn clamp_vec2(p: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
}
