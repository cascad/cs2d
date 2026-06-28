//! Клиентская часть тумана войны.
//!
//! Безопасность обеспечивается СЕРВЕРОМ (он не присылает невидимых игроков —
//! см. `server_tick`). Здесь — только визуал поверх этого:
//!   1. `fade_unseen_players` — плавно гасит и деспавнит тех, кого сервер
//!      перестал присылать (вышли из зоны видимости);
//!   2. `setup_fog`/`update_fog` — «диабловское» затемнение карты на основе
//!      ТЕКСТУРЫ тумана (по тайлу карты на тексель). Каждый кадр для каждого
//!      текселя считаем LOS до игрока (per-tile shadowcasting), сглаживаем
//!      темпорально (никаких рывков при повороте) и пишем альфу в текстуру.
//!      Билинейная фильтрация спрайта даёт мягкую границу, а «исследованные,
//!      но сейчас невидимые» тайлы остаются приглушённо видны (память карты).

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::AlphaMode2d;

use crate::components::{LocalPlayer, PlayerMarker};
use crate::render::{layers, world_to_screen};
use crate::resources::{LastSeen, MyPlayer, SpawnedPlayers, WallGridRes};
use crate::systems::level_fixed::{map_dims, TILE};
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

// ---------------------------------------------------------------------------
// Затемнение карты через текстуру тумана (1 тексель = 1 тайл карты).
// ---------------------------------------------------------------------------

// Туман войны (НЕ чёрный, серый): чётко виден «водораздел» между тем, что игрок
// сейчас видит, и тем, что вне обзора. Видно сейчас — прозрачно; край обзора —
// мягко гаснет; вне обзора/за стеной — заметно приглушено серым (но различимо).
const DARK_VISIBLE: f32 = 0.0; // в куполе обзора и без стены на линии — без затемнения
const DARK_EXPLORED: f32 = 0.45; // видели раньше / вне конуса — приглушено, но различимо
const DARK_HIDDEN: f32 = 0.75; // никогда не видели — тёмно, но не чёрная дыра

/// Ближний радиус (мир. ед.), где видно ВСЕГДА, даже вне конуса зрения — игрок
/// «чувствует» то, что прямо у ног/за спиной вплотную. Небольшой.
const FOG_NEAR_RADIUS: f32 = 96.0;
/// Радиус «полной видимости» (мир. ед.): внутри — без затемнения.
const FOG_VIEW_FULL: f32 = 440.0;
/// Радиус края обзора (мир. ед.): от `FOG_VIEW_FULL` до него купол мягко гаснет;
/// дальше считаем «не вижу». Меньше серверного `VIEW_RADIUS` (тот — про анти-чит
/// куллинг по сети), чтобы граница купола РЕАЛЬНО попадала на экран и была видна.
const FOG_VIEW_FADE: f32 = 820.0;

/// Цвет тумана (RGB), нейтрально-серый с лёгкой прохладцей: «невидимое» выглядит
/// серым (по просьбе), а не чёрным и не синим. Заливается один раз; каждый кадр
/// меняем только альфу.
const FOG_RGB: [u8; 3] = [22, 23, 27];
/// Скорость темпорального сглаживания (1/сек). Чем больше — тем резче переходы.
const FOG_SMOOTH_K: f32 = 12.0;
/// Сжатие стен при LOS, чтобы луч по грани не давал ложного перекрытия.
const FOG_EPS: f32 = 0.5;

#[derive(Component)]
pub struct FogOverlay;

/// Состояние тумана: дискретная сетка по тайлам карты + сглаженная «темнота» и
/// память «исследованного». Хранится отдельно от текстуры, чтобы сглаживать на CPU.
#[derive(Resource)]
pub struct FogState {
    handle: Handle<Image>,
    cols: usize,
    rows: usize,
    map_w: f32,
    map_h: f32,
    darkness: Vec<f32>,
    explored: Vec<bool>,
}

impl FogState {
    /// Мировой центр текселя (ix, iy). iy=0 — верхний ряд (макс. world.y), как в спрайте.
    #[inline]
    fn texel_world(&self, ix: usize, iy: usize) -> Vec2 {
        let x = -self.map_w * 0.5 + (ix as f32 + 0.5) * TILE;
        let y = self.map_h * 0.5 - (iy as f32 + 0.5) * TILE;
        Vec2::new(x, y)
    }
}

pub fn setup_fog(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (cols, rows) = map_dims();
    let map_w = cols as f32 * TILE;
    let map_h = rows as f32 * TILE;
    let n = cols * rows;

    // Изначально вся карта «не исследована». RGB — холодный оттенок тумана,
    // альфа — степень затемнения.
    let init_alpha = (DARK_HIDDEN * 255.0) as u8;
    let mut data = vec![0u8; n * 4];
    for px in data.chunks_exact_mut(4) {
        px[0] = FOG_RGB[0];
        px[1] = FOG_RGB[1];
        px[2] = FOG_RGB[2];
        px[3] = init_alpha;
    }

    let mut image = Image::new(
        Extent3d {
            width: cols as u32,
            height: rows as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // Билинейная фильтрация → мягкая граница тумана при апскейле текселя до тайла.
    image.sampler = ImageSampler::linear();

    let handle = images.add(image);

    // Туман натягиваем на изо-ромб карты (как пол), а не на экранный
    // прямоугольник: тогда затемнение совпадает с проекцией пола/стен.
    // UV: u по world.x слева-направо, v по world.y сверху-вниз (iy=0 — верхний ряд).
    let hw = map_w * 0.5;
    let hh = map_h * 0.5;
    let corners = [
        (Vec2::new(-hw, hh), [0.0, 0.0]),
        (Vec2::new(hw, hh), [1.0, 0.0]),
        (Vec2::new(hw, -hh), [1.0, 1.0]),
        (Vec2::new(-hw, -hh), [0.0, 1.0]),
    ];
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(4);
    for (w, uv) in corners {
        let s = world_to_screen(w);
        positions.push([s.x, s.y, 0.0]);
        uvs.push(uv);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    let mesh = meshes.add(mesh);

    let mat = materials.add(ColorMaterial {
        color: Color::WHITE,
        texture: Some(handle.clone()),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    });

    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, layers::FOG),
        FogOverlay,
    ));

    commands.insert_resource(FogState {
        handle,
        cols,
        rows,
        map_w,
        map_h,
        darkness: vec![DARK_HIDDEN; n],
        explored: vec![false; n],
    });
}

/// Каждый кадр пересчитываем видимость по тайлам (LOS до игрока), сглаживаем
/// темпорально и заливаем альфу в текстуру тумана.
pub fn update_fog(
    player_q: Query<&crate::render::WorldPos, With<LocalPlayer>>,
    aim: Res<crate::resources::AimAngle>,
    walls: Res<WallGridRes>,
    time: Res<Time>,
    mut fog: ResMut<FogState>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(wp) = player_q.single() else { return };
    let player = wp.0;
    // Конус зрения смотрит туда, КУДА ЦЕЛИТСЯ КУРСОР (мгновенно), а не куда уже
    // довернулась модель — иначе «светлая зона» не совпадала с вниманием игрока.
    let face = aim.0;

    let dt = time.delta_secs();
    let smooth = 1.0 - (-FOG_SMOOTH_K * dt).exp();
    let fade2 = FOG_VIEW_FADE * FOG_VIEW_FADE;
    let near2 = FOG_NEAR_RADIUS * FOG_NEAR_RADIUS;
    let span = (FOG_VIEW_FADE - FOG_VIEW_FULL).max(1.0);
    // LOS целимся чуть «не доходя» до центра текселя, чтобы грани стен (их центр за
    // ближней гранью) считались видимыми, а не висели чёрными.
    let pull = TILE * 0.6;

    let cols = fog.cols;
    let rows = fog.rows;

    for iy in 0..rows {
        for ix in 0..cols {
            let i = iy * cols + ix;
            let center = fog.texel_world(ix, iy);
            let to = center - player;
            let dist2 = to.length_squared();

            // «Сейчас вижу» = внутри края обзора, В КОНУСЕ зрения (не за спиной) И
            // нет стены на линии взгляда.
            let in_cone = dist2 <= near2
                || protocol::combat::in_fov(player, face, center, protocol::constants::VIEW_FOV_HALF_ANGLE);
            let visible = if dist2 <= fade2 && in_cone {
                let dist = dist2.sqrt();
                let test = if dist > pull {
                    center - to / dist * pull
                } else {
                    player
                };
                !walls.0.segment_blocked(player, test, FOG_EPS)
            } else {
                false
            };

            if visible {
                fog.explored[i] = true;
            }
            let target = if visible {
                // мягкий купол: в центре прозрачно, к краю обзора плавно гаснет до
                // DARK_EXPLORED → виден чёткий «водораздел» вижу/не вижу
                let dist = dist2.sqrt();
                let t = ((dist - FOG_VIEW_FULL) / span).clamp(0.0, 1.0);
                DARK_VISIBLE + t * (DARK_EXPLORED - DARK_VISIBLE)
            } else if fog.explored[i] {
                DARK_EXPLORED
            } else {
                DARK_HIDDEN
            };
            fog.darkness[i] += (target - fog.darkness[i]) * smooth;
        }
    }

    // Заливаем в текстуру (изменение через get_mut триггерит реаплоад на GPU).
    if let Some(img) = images.get_mut(&fog.handle) {
        if let Some(buf) = img.data.as_mut() {
            for (i, d) in fog.darkness.iter().enumerate() {
                let a = (d.clamp(0.0, 1.0) * 255.0) as u8;
                let o = i * 4;
                buf[o + 3] = a;
            }
        }
    }
}
