// camera_follow.rs
use bevy::prelude::*;
use bevy::camera::{Projection, ScalingMode};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::window::PrimaryWindow;

use crate::app_state::AppState;
use crate::components::PlayerMarker;
use crate::resources::MyPlayer;
use crate::systems::level_fixed::{TILE, map_dims};

/// Зум камеры (масштаб орто-проекции). Меньше — ближе. Текущее «далеко» = 1.0 —
/// это максимальное отдаление; колесо мыши приближает до `ZOOM_MIN`.
pub const ZOOM_MIN: f32 = 0.45;
pub const ZOOM_MAX: f32 = 1.0;
const ZOOM_STEP: f32 = 0.08;

#[derive(Resource, Clone, Copy)]
pub struct CameraZoom {
    pub scale: f32,
}
impl Default for CameraZoom {
    fn default() -> Self {
        Self { scale: ZOOM_MAX }
    }
}

/// Колесо мыши приближает/отдаляет камеру в пределах [`ZOOM_MIN`, `ZOOM_MAX`].
fn camera_zoom_input(mut wheel: MessageReader<MouseWheel>, mut zoom: ResMut<CameraZoom>) {
    let mut delta = 0.0;
    for ev in wheel.read() {
        // строки колеса считаем «как есть», пиксели нормируем
        let amount = match ev.unit {
            MouseScrollUnit::Line => ev.y,
            MouseScrollUnit::Pixel => ev.y / 50.0,
        };
        delta += amount;
    }
    if delta != 0.0 {
        // вверх (delta>0) = приближаем (scale меньше)
        zoom.scale = (zoom.scale - delta * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

// твои типы/функции – поправь путь, если нужны модули:

#[derive(Resource, Debug, Clone, Copy)]
pub struct LevelBounds {
    pub min: Vec2,
    pub max: Vec2,
}

pub struct CameraFollowPlugin;

impl Plugin for CameraFollowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFollowSettings>() // ← настройки по умолчанию
            .init_resource::<CameraZoom>()
            .add_systems(OnEnter(AppState::InGame), init_level_bounds)
            .add_systems(
                Update,
                camera_zoom_input.run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                PostUpdate,
                follow_player_camera_smooth.run_if(in_state(AppState::InGame)),
            );
    }
}

#[derive(Resource, Clone)]
pub struct CameraFollowSettings {
    /// доля размеров экрана для "мёртвой зоны"
    pub deadzone_frac: Vec2, // напр. Vec2::new(0.30, 0.35)
    /// скорость сглаживания (чем больше — тем быстрее догоняет)
    pub follow_lerp: f32, // напр. 10.0
    /// сколько секунд "заглядывать вперёд" по скорости игрока
    pub lookahead_time: f32, // напр. 0.15
    /// ограничение look-ahead как доля half_view
    pub max_lookahead_frac: f32, // напр. 0.25
}
impl Default for CameraFollowSettings {
    fn default() -> Self {
        Self {
            // без «мёртвой зоны»: камера держит игрока по центру, поэтому со всех
            // сторон видно одинаково и обзор смещается сразу при движении
            deadzone_frac: Vec2::new(0.0, 0.0),
            follow_lerp: 20.0,
            lookahead_time: 0.0,
            max_lookahead_frac: 0.0,
        }
    }
}

fn init_level_bounds(mut commands: Commands) {
    use crate::render::world_to_screen;

    let (cols, rows) = map_dims();
    let w = cols as f32;
    let h = rows as f32;

    let half = Vec2::new(w * TILE, h * TILE) * 0.5;

    // Камера живёт в ЭКРАННЫХ координатах (за ней едет `Transform`, который
    // выставляет изо-проекция). Поэтому границы — это экранный AABB ромба карты:
    // проецируем 4 угла и берём min/max.
    let corners = [
        world_to_screen(Vec2::new(-half.x, -half.y)),
        world_to_screen(Vec2::new(half.x, -half.y)),
        world_to_screen(Vec2::new(half.x, half.y)),
        world_to_screen(Vec2::new(-half.x, half.y)),
    ];
    let mut min = corners[0];
    let mut max = corners[0];
    for c in corners.iter().skip(1) {
        min = min.min(*c);
        max = max.max(*c);
    }
    commands.insert_resource(LevelBounds { min, max });
}

fn follow_player_camera_smooth(
    me: Res<MyPlayer>,
    bounds: Res<LevelBounds>,
    settings: Res<CameraFollowSettings>,
    zoom: Res<CameraZoom>,
    time: Res<Time>,
    q_win: Query<&Window, With<PrimaryWindow>>,

    // камера: мутируем; доказываем дизъюнктность с игроками
    mut q_cam: Query<(&mut Projection, &mut Transform), (With<Camera2d>, Without<PlayerMarker>)>,

    // игроки: читаем трансформы
    q_players: Query<(&Transform, &PlayerMarker), (With<PlayerMarker>, Without<Camera2d>)>,

    // локальное состояние для оценки скорости игрока
    mut last_player_pos: Local<Option<Vec2>>,
) {
    let Ok((mut proj, mut cam_tf)) = q_cam.single_mut() else {
        return;
    };
    let Ok(win) = q_win.single() else {
        return;
    };

    // найдём локального
    let mut player_pos: Option<Vec2> = None;
    for (tf, pm) in &q_players {
        if pm.0 == me.id {
            player_pos = Some(tf.translation.truncate());
            break;
        }
    }
    let Some(p) = player_pos else {
        return;
    };

    // half-view в мировых единицах при орто-проекции; масштаб берём из зума
    let half_view = if let Projection::Orthographic(ortho) = &mut *proj {
        ortho.scaling_mode = ScalingMode::WindowSize;
        ortho.scale = zoom.scale.clamp(ZOOM_MIN, ZOOM_MAX);
        Vec2::new(win.width(), win.height()) * ortho.scale * 0.5
    } else {
        return;
    };

    // --- look-ahead по скорости (из дельты позиций) ---
    let mut target = p;
    if let Some(prev) = *last_player_pos {
        let v = (p - prev) / time.delta_secs().max(1e-6);
        let la = v * settings.lookahead_time;
        let max_la = Vec2::splat(half_view.min_element() * settings.max_lookahead_frac);
        target += la.clamp(-max_la, max_la);
    }
    *last_player_pos = Some(p);

    // --- мёртвая зона вокруг центра камеры ---
    let mut cam = cam_tf.translation.truncate();
    let dz_half = half_view * settings.deadzone_frac;
    let dz_min = cam - dz_half;
    let dz_max = cam + dz_half;

    if target.x < dz_min.x {
        cam.x = target.x + dz_half.x;
    } else if target.x > dz_max.x {
        cam.x = target.x - dz_half.x;
    }
    if target.y < dz_min.y {
        cam.y = target.y + dz_half.y;
    } else if target.y > dz_max.y {
        cam.y = target.y - dz_half.y;
    }

    // --- сглаживание (экспоненциальное приближение) ---
    let smooth = 1.0 - (-settings.follow_lerp * time.delta_secs()).exp();
    let desired = cam;
    let new_pos = cam_tf.translation.truncate().lerp(desired, smooth);

    // --- клаймп по реальным границам уровня ---
    let min_allowed = bounds.min + half_view;
    let max_allowed = bounds.max - half_view;

    let clamped = Vec2::new(
        if min_allowed.x > max_allowed.x {
            (bounds.min.x + bounds.max.x) * 0.5
        } else {
            new_pos.x.clamp(min_allowed.x, max_allowed.x)
        },
        if min_allowed.y > max_allowed.y {
            (bounds.min.y + bounds.max.y) * 0.5
        } else {
            new_pos.y.clamp(min_allowed.y, max_allowed.y)
        },
    );

    cam_tf.translation.x = clamped.x;
    cam_tf.translation.y = clamped.y;
}
