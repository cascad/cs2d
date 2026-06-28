use crate::components::{ExplosionFx, Grenade};
use crate::events::GrenadeDetonatedEvent;
use crate::render::{layers, world_to_screen, world_to_translation};
use crate::resources::WallGridRes;
use crate::systems::grenade_lifecycle::make_screen_disc_mesh;
use crate::ui::components::ExplosionMaterial;
use bevy::asset::RenderAssetUsages;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use protocol::constants::GRENADE_BLAST_RADIUS;
use protocol::geom::WallGrid;

// ------------------------------------------------------------------------------------------------
// Рендер детонаций по серверному событию.
//
// Взрыв собираем из трёх анимированных слоёв, чтобы плоский круг стал «объёмным»:
//   1) наземная ударная волна — изо-эллипс, ОБРЕЗАННЫЙ стенами (тот самый радиус,
//      что нравится: он честно показывает зону поражения). Лежит на полу, растёт.
//   2) огненный шар — круглый экранный диск, который раздувается и ПОДНИМАЕТСЯ.
//   3) яркая вспышка-ядро — короткая белёсая засветка в центре.
// ------------------------------------------------------------------------------------------------
pub fn render_detonations(
    mut commands: Commands,
    mut evr: MessageReader<GrenadeDetonatedEvent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    q_gren: Query<(Entity, &Grenade)>,
    wall_grid: Res<WallGridRes>,
) {
    for e in evr.read() {
        // снять летящий пузырёк
        if let Some((ent, _)) = q_gren.iter().find(|(_, g)| g.id == e.id) {
            commands.entity(ent).despawn();
        }

        // --- 1) Наземная ударная волна (изо, обрезана стенами) ---
        let ground_mesh = meshes.add(generate_occluded_explosion_mesh(
            e.pos,
            GRENADE_BLAST_RADIUS,
            96,
            &wall_grid.0,
        ));
        spawn_fx_layer(
            &mut commands,
            ground_mesh,
            &mut materials,
            Color::srgba(1.0, 0.42, 0.12, 1.0),
            world_to_translation(e.pos, layers::EFFECT),
            ExplosionFx {
                timer: Timer::from_seconds(0.5, TimerMode::Once),
                base: world_to_translation(e.pos, layers::EFFECT),
                scale_from: 0.55,
                scale_to: 1.0,
                rise: 0.0,
                alpha_from: 0.55,
            },
        );

        // --- 2) Огненный шар (раздувается и поднимается) ---
        let fire_mesh = meshes.add(make_screen_disc_mesh(GRENADE_BLAST_RADIUS * 0.42, 32));
        let mut fire_base = world_to_translation(e.pos, layers::EFFECT);
        fire_base.z += 0.10; // над наземной волной
        spawn_fx_layer(
            &mut commands,
            fire_mesh,
            &mut materials,
            Color::srgba(1.0, 0.55, 0.16, 1.0),
            fire_base,
            ExplosionFx {
                timer: Timer::from_seconds(0.42, TimerMode::Once),
                base: fire_base,
                scale_from: 0.35,
                scale_to: 1.05,
                rise: 26.0,
                alpha_from: 0.9,
            },
        );

        // --- 3) Яркая вспышка-ядро (короткая) ---
        let flash_mesh = meshes.add(make_screen_disc_mesh(GRENADE_BLAST_RADIUS * 0.26, 24));
        let mut flash_base = world_to_translation(e.pos, layers::EFFECT);
        flash_base.z += 0.20; // поверх шара
        spawn_fx_layer(
            &mut commands,
            flash_mesh,
            &mut materials,
            Color::srgba(1.0, 0.95, 0.72, 1.0),
            flash_base,
            ExplosionFx {
                timer: Timer::from_seconds(0.20, TimerMode::Once),
                base: flash_base,
                scale_from: 0.5,
                scale_to: 1.4,
                rise: 34.0,
                alpha_from: 1.0,
            },
        );
    }
}

/// Спавн одного анимированного слоя взрыва.
fn spawn_fx_layer(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    mut color: Color,
    base: Vec3,
    fx: ExplosionFx,
) {
    color.set_alpha(fx.alpha_from);
    let material = materials.add(ColorMaterial {
        color,
        alpha_mode: AlphaMode2d::Blend.into(),
        uv_transform: Affine2::IDENTITY,
        texture: None,
    });
    let mat_handle = material.clone();

    // ВАЖНО: у взрыва НЕТ `WorldPos`/`RenderLayer` — это статичный наземный эффект,
    // чью экранную позицию мы уже посчитали в `base` (как у декали удара). Иначе
    // `project_world_to_transform` каждый кадр перепроецировал бы его и затирал
    // анимацию — взрыв «уезжал» бы не туда.
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_translation(base).with_scale(Vec3::splat(fx.scale_from)),
        GlobalTransform::default(),
        Visibility::Visible,
        InheritedVisibility::default(),
        ViewVisibility::default(),
        fx,
        ExplosionMaterial(mat_handle),
    ));
}

/// Анимация слоёв взрыва: масштаб + подъём + затухание, затем despawn.
pub fn animate_explosion_fx(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(Entity, &mut ExplosionFx, &mut Transform, &ExplosionMaterial)>,
) {
    for (ent, mut fx, mut tf, mat) in q.iter_mut() {
        fx.timer.tick(time.delta());
        let t = (fx.timer.elapsed_secs() / fx.timer.duration().as_secs_f32()).clamp(0.0, 1.0);
        // ease-out для расширения/подъёма (быстро в начале, мягко в конце)
        let ease = 1.0 - (1.0 - t) * (1.0 - t);

        let scale = fx.scale_from + (fx.scale_to - fx.scale_from) * ease;
        tf.scale = Vec3::splat(scale);
        tf.translation.x = fx.base.x;
        tf.translation.y = fx.base.y + fx.rise * ease;
        tf.translation.z = fx.base.z;

        if let Some(m) = materials.get_mut(&mat.0) {
            m.color.set_alpha(fx.alpha_from * (1.0 - t));
        }

        if fx.timer.is_finished() {
            commands.entity(ent).despawn();
        }
    }
}

// ---------- Генерация «обрезанного» меша взрыва (треугольный фан, ИЗО) ----------
//
// Лучи raycast'ятся по МИРУ (честная обрезка стенами), а вершины проецируются в
// изометрию (`world_to_screen`), поэтому зона поражения ложится на пол эллипсом,
// а не плоским кругом. Центр — в (0,0); сущность ставится в экранную точку взрыва.
fn generate_occluded_explosion_mesh(
    center: Vec2,
    radius: f32,
    segments: usize,
    wall_grid: &WallGrid,
) -> Mesh {
    let mut positions = Vec::with_capacity(2 + segments);
    let mut indices = Vec::with_capacity(segments * 3);

    positions.push([0.0, 0.0, 0.0]); // центр

    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let theta = t * std::f32::consts::TAU;
        let dir = Vec2::new(theta.cos(), theta.sin());

        let d = wall_grid.raycast(center, dir, radius); // обрезка стенами (в мире)
        let s = world_to_screen(dir * d); // → изо-смещение от центра
        positions.push([s.x, s.y, 0.0]);
    }

    for i in 1..=segments {
        indices.extend([0u32, i as u32, (i + 1) as u32]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
