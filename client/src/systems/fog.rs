//! Клиентская часть тумана войны.
//!
//! Безопасность обеспечивается СЕРВЕРОМ (он не присылает невидимых игроков —
//! см. `server_tick`). Здесь — только визуал поверх этого:
//!   1. `fade_unseen_players` — плавно гасит и деспавнит тех, кого сервер
//!      перестал присылать (вышли из зоны видимости);
//!   2. `setup_fog`/`update_fog`/`apply_fog_tint` — «диабловское» затемнение
//!      карты. Каждый кадр считаем LOS-конус по тайлам (видно/исследовано/не
//!      виданное), сглаживаем темпорально, а затем УМНОЖАЕМ цвет спрайтов тайлов
//!      (пол/стены/пропы) на яркость = 1−darkness. Тинтим сами спрайты карты —
//!      эффект гарантированно виден (прежний оверлей-mesh не композился в этом
//!      рендер-пути и тумана на экране не было).

use bevy::prelude::*;

use crate::components::{LocalPlayer, PlayerMarker};
use crate::resources::{HpUiMap, LastSeen, MyPlayer, SpawnedPlayers, VisionGridRes};
use crate::systems::level_fixed::{map_dims, TILE};
use crate::systems::utils::time_in_seconds;

// --- Мгновенное скрытие пропавших из виду ---
// Сервер режет снапшот по полю зрения: как только цель уходит из обзора, она
// перестаёт приходить. Показывать «фантом» после этого нельзя (по просьбе) —
// деспавним сразу. Крошечный порог (~интерп.-задержка + 1 тик) лишь сглаживает
// одиночный потерянный пакет, чтобы не было мерцания, но на глаз это «мгновенно».
pub(crate) const FADE_START: f64 = 0.0;
pub(crate) const FADE_END: f64 = 0.10;

/// Гасит альфу игроков (вместе с дочерним «стволом») по времени с последнего
/// появления в снапшоте; полностью пропавших деспавнит и снимает с учёта, чтобы
/// при повторном появлении они заспавнились заново.
pub fn fade_unseen_players(
    mut commands: Commands,
    my: Res<MyPlayer>,
    mut last_seen: ResMut<LastSeen>,
    mut spawned: ResMut<SpawnedPlayers>,
    mut hp_ui: ResMut<HpUiMap>,
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
        // плавающая полоска HP — ОТДЕЛЬНАЯ сущность (не ребёнок игрока): без этого
        // она «зависала в воздухе» там, где игрок пропал из виду. Снимаем вместе.
        if let Some(ui) = hp_ui.0.remove(&id) {
            commands.entity(ui).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Туман войны через ПРЯМОЙ ТИНТ спрайтов карты (пол/стены/пропы), без отдельного
// оверлея. Раньше затемнение рисовалось текстурой на mesh поверх сцены, но в
// этом рендер-пути overlay не композился (на экране эффекта не было). Тут мы
// считаем «темноту» по тайлам (LOS-конус + память), а затем УМНОЖАЕМ цвет
// каждого спрайта тайла на яркость — это гарантированно видно, т.к. меняем сами
// спрайты, которые точно отрисовываются.
// ---------------------------------------------------------------------------

// Множитель ЯРКОСТИ = 1 - darkness. В конусе — полная яркость; вне конуса/за
// стеной — заметно темнее (чтобы «водораздел» вижу/не-вижу читался сразу). Это
// главные ручки силы тумана.
const DARK_VISIBLE: f32 = 0.0; // в конусе и без стены на линии — без затемнения (яркость ×1.0)
const DARK_EXPLORED: f32 = 0.5; // видели раньше / вне конуса — ×0.5 (явно темнее)
const DARK_HIDDEN: f32 = 0.72; // никогда не видели — ×0.28 (тёмно, но различимо)

/// Ближний радиус (мир. ед.), где видно ВСЕГДА, даже вне конуса зрения — игрок
/// «чувствует» то, что прямо у ног/за спиной вплотную. Совпадает с серверным
/// `NEAR_SIGHT_RADIUS`, чтобы подсветка тайлов и реально присланные враги сошлись.
const FOG_NEAR_RADIUS: f32 = protocol::constants::NEAR_SIGHT_RADIUS;
/// Радиус «полной видимости» (мир. ед.): внутри — без затемнения.
const FOG_VIEW_FULL: f32 = 460.0;
/// Радиус края обзора (мир. ед.): от `FOG_VIEW_FULL` до него купол мягко гаснет;
/// дальше считаем «не вижу». Меньше серверного `VIEW_RADIUS` (тот — про анти-чит
/// куллинг по сети), чтобы граница купола РЕАЛЬНО попадала на экран и была видна.
const FOG_VIEW_FADE: f32 = 900.0;

/// Скорость темпорального сглаживания (1/сек). Чем больше — тем резче переходы.
const FOG_SMOOTH_K: f32 = 12.0;
/// Сжатие стен при LOS, чтобы луч по грани не давал ложного перекрытия.
const FOG_EPS: f32 = 0.5;

/// Базовый цвет спрайта тайла (как при спавне) и его мировой центр. Туман каждый
/// кадр умножает базовый цвет на яркость видимости в этой точке. База хранится,
/// чтобы повторный множитель не «съедал» цвет; позиция — чтобы не зависеть от
/// `WorldPos` (у пола/стен его нет, а добавление сломало бы их Z-слой).
#[derive(Component, Clone, Copy)]
pub struct FogTint {
    pub base: Srgba,
    pub pos: Vec2,
}

impl FogTint {
    #[inline]
    pub fn new(color: Color, pos: Vec2) -> Self {
        Self { base: color.to_srgba(), pos }
    }
}

/// Состояние тумана: дискретная сетка по тайлам карты со сглаженной «темнотой» и
/// памятью «исследованного». Сглаживание — на CPU, затем читается тинт-системой.
#[derive(Resource)]
pub struct FogState {
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

    /// «Темнота» (0..1) в точке мира — по тайлу, в котором она лежит. Вне карты —
    /// считаем максимально тёмной (как «никогда не виданное»).
    #[inline]
    fn darkness_at(&self, world: Vec2) -> f32 {
        let fx = (world.x + self.map_w * 0.5) / TILE;
        let fy = (self.map_h * 0.5 - world.y) / TILE;
        if fx < 0.0 || fy < 0.0 {
            return DARK_HIDDEN;
        }
        let (ix, iy) = (fx as usize, fy as usize);
        if ix >= self.cols || iy >= self.rows {
            return DARK_HIDDEN;
        }
        self.darkness[iy * self.cols + ix]
    }
}

pub fn setup_fog(mut commands: Commands) {
    let (cols, rows) = map_dims();
    let map_w = cols as f32 * TILE;
    let map_h = rows as f32 * TILE;
    let n = cols * rows;

    commands.insert_resource(FogState {
        cols,
        rows,
        map_w,
        map_h,
        darkness: vec![DARK_HIDDEN; n],
        explored: vec![false; n],
    });
}

/// Каждый кадр пересчитываем видимость по тайлам (LOS до игрока) и сглаживаем
/// темпорально. Результат (сетка «темноты») читает [`apply_fog_tint`].
pub fn update_fog(
    player_q: Query<&crate::render::WorldPos, With<LocalPlayer>>,
    aim: Res<crate::resources::AimAngle>,
    vision: Res<VisionGridRes>,
    time: Res<Time>,
    mut fog: ResMut<FogState>,
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
                !vision.0.segment_blocked(player, test, FOG_EPS)
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
}

/// Туман-войны как ТИНТ: каждый кадр умножаем базовый цвет спрайта тайла (пол/
/// стены/пропы) на яркость = 1 − darkness в его тайле. В конусе — полная яркость,
/// вне — приглушённо. Меняем сами спрайты карты, поэтому эффект гарантированно
/// виден (в отличие от прежнего оверлея, который не композился).
pub fn apply_fog_tint(fog: Res<FogState>, mut q: Query<(&FogTint, &mut Sprite)>) {
    for (tint, mut sprite) in q.iter_mut() {
        let b = (1.0 - fog.darkness_at(tint.pos)).clamp(0.0, 1.0);
        let base = tint.base;
        sprite.color = Color::srgba(base.red * b, base.green * b, base.blue * b, base.alpha);
    }
}
