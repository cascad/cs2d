// camera_follow.rs
use bevy::prelude::*;
use bevy::render::camera::{Projection, ScalingMode};
use bevy::window::PrimaryWindow;

use crate::app_state::AppState;
use crate::components::PlayerMarker;
use crate::resources::MyPlayer;
use crate::systems::level_fixed::{TILE, map_lines};

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
            .add_systems(OnEnter(AppState::InGame), init_level_bounds)
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
            deadzone_frac: Vec2::new(0.30, 0.35),
            follow_lerp: 10.0,
            lookahead_time: 0.15,
            max_lookahead_frac: 0.25,
        }
    }
}

fn init_level_bounds(mut commands: Commands) {
    let lines = map_lines();
    let h = lines.len() as f32;
    let w = lines
        .iter()
        .map(|s| s.chars().count() as f32)
        .max_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0);

    let size = Vec2::new(w * TILE, h * TILE);
    let half = size * 0.5;

    // было: min = (0,0), max = (w*TILE, h*TILE)
    commands.insert_resource(LevelBounds {
        min: -half, // (-w/2, -h/2)
        max: half,  // ( w/2,  h/2)
    });
}

fn follow_player_camera_hard(
    me: Res<MyPlayer>,
    bounds: Res<LevelBounds>,
    q_win: Query<&Window, With<PrimaryWindow>>,

    // Берём единственную 2D-камеру, но исключаем игроков (иначе B0001)
    mut q_cam: Query<(&mut Projection, &mut Transform), (With<Camera2d>, Without<PlayerMarker>)>,

    // Ищем всех игроков, найдём по id
    q_players: Query<(&Transform, &PlayerMarker)>,
) {
    let Ok((mut proj, mut cam_tf)) = q_cam.single_mut() else {
        return;
    };
    let Ok(win) = q_win.single() else {
        return;
    };

    // найдём локального игрока по id
    let mut target: Option<Vec2> = None;
    for (tf, pm) in &q_players {
        if pm.0 == me.id {
            target = Some(tf.translation.truncate());
            break;
        }
    }

    let Some(target) = target else {
        // на всякий: раскомментируй для диагностики
        // info!("camera: no player with id={} yet", me.id);
        return;
    };

    // Включаем «пиксельную» шкалу, вычисляем половину видимой области
    let half_view = if let Projection::Orthographic(ortho) = &mut *proj {
        ortho.scaling_mode = ScalingMode::WindowSize;
        // гарантируем адекватный стартовый масштаб
        if ortho.scale < 1.0 {
            ortho.scale = 1.0;
        }
        Vec2::new(win.width(), win.height()) * ortho.scale * 0.5
    } else {
        return;
    };

    // Держим камеру внутри уровня с учётом half_view
    let min_allowed = bounds.min + half_view;
    let max_allowed = bounds.max - half_view;

    let clamped = Vec2::new(
        if min_allowed.x > max_allowed.x {
            (bounds.min.x + bounds.max.x) * 0.5
        } else {
            target.x.clamp(min_allowed.x, max_allowed.x)
        },
        if min_allowed.y > max_allowed.y {
            (bounds.min.y + bounds.max.y) * 0.5
        } else {
            target.y.clamp(min_allowed.y, max_allowed.y)
        },
    );

    // жёстко ставим камеру
    cam_tf.translation.x = clamped.x;
    cam_tf.translation.y = clamped.y;

    // Диагностика (включи, если надо)
    info!(
        "cam-> player_id={} target=({:.1},{:.1}) cam=({:.1},{:.1}) hv=({:.1},{:.1})",
        me.id,
        target.x,
        target.y,
        cam_tf.translation.x,
        cam_tf.translation.y,
        half_view.x,
        half_view.y
    );
}

fn snap_camera_on_player_spawn(
    me: Res<MyPlayer>,
    mut did: Local<bool>,
    mut q_cam: Query<&mut Transform, (With<Camera2d>, Without<PlayerMarker>)>,
    q_players: Query<(&Transform, &PlayerMarker)>,
) {
    if *did {
        return;
    }
    let Ok(mut cam_tf) = q_cam.get_single_mut() else {
        return;
    };
    for (tf, pm) in &q_players {
        if pm.0 == me.id {
            cam_tf.translation.x = tf.translation.x;
            cam_tf.translation.y = tf.translation.y;
            *did = true;
            info!("Camera snapped to player {}", me.id);
            break;
        }
    }
}

fn follow_player_camera_smooth(
    me: Res<MyPlayer>,
    bounds: Res<LevelBounds>,
    settings: Res<CameraFollowSettings>,
    time: Res<Time>,
    q_win: Query<&Window, With<PrimaryWindow>>,

    // камера: мутируем; доказываем дизъюнктность с игроками
    mut q_cam: Query<(&mut Projection, &mut Transform), (With<Camera2d>, Without<PlayerMarker>)>,

    // игроки: читаем трансформы
    q_players: Query<(&Transform, &PlayerMarker), (With<PlayerMarker>, Without<Camera2d>)>,

    // локальное состояние для оценки скорости игрока
    mut last_player_pos: Local<Option<Vec2>>,
) {
    let Ok((mut proj, mut cam_tf)) = q_cam.get_single_mut() else {
        return;
    };
    let Ok(win) = q_win.get_single() else {
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

    // half-view в мировых единицах при орто-проекции
    let half_view = if let Projection::Orthographic(ortho) = &mut *proj {
        ortho.scaling_mode = ScalingMode::WindowSize;
        if ortho.scale < 1.0 {
            ortho.scale = 1.0;
        }
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
