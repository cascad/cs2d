//! Клиентская часть тумана войны: затемнение карты по LOS-конусу игрока.
//! Невидимые сущности отсекаются сервером через `NetworkVisibility` (Lightyear).

use bevy::prelude::*;

use crate::components::LocalPlayer;
use crate::resources::VisionGridRes;
use crate::systems::level_fixed::{map_dims, TILE};

pub(crate) const FADE_END: f64 = 0.10;

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

/// Порог перерасчёта видимости: сдвиг игрока (мир. ед.) / поворот прицела (рад).
/// LOS-рейкасты по всем тайлам — самая дорогая часть тумана; пока игрок стоит и
/// не крутит прицел, пересчитывать нечего (сглаживание продолжает работать).
const FOG_RECALC_MOVE: f32 = 3.0;
const FOG_RECALC_TURN: f32 = 0.03;
/// Страховочный период пересчёта (сек) при полной неподвижности.
const FOG_RECALC_PERIOD: f32 = 0.25;

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
    /// Целевая «темнота» тайла из последнего LOS-пересчёта; darkness каждый кадр
    /// сглаживается к ней (дёшево), сам пересчёт — только по движению/таймеру.
    targets: Vec<f32>,
    explored: Vec<bool>,
    last_pos: Vec2,
    last_aim: f32,
    since_recalc: f32,
}

impl FogState {
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
        targets: vec![DARK_HIDDEN; n],
        explored: vec![false; n],
        last_pos: Vec2::splat(f32::MAX), // гарантирует пересчёт первым кадром
        last_aim: 0.0,
        since_recalc: 0.0,
    });
}

/// Видимость по тайлам (LOS до игрока). Дорогая часть — рейкасты по всем тайлам
/// — выполняется только когда игрок сдвинулся/повернул прицел (или раз в
/// `FOG_RECALC_PERIOD` для страховки); результат кэшируется в `targets`.
/// Дешёвое сглаживание `darkness → targets` идёт каждый кадр; сетку читает
/// [`apply_fog_tint`].
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
    fog.since_recalc += dt;
    let turn = {
        use core::f32::consts::{PI, TAU};
        ((face - fog.last_aim + PI).rem_euclid(TAU) - PI).abs()
    };
    let need_recalc = fog.last_pos.distance_squared(player) > FOG_RECALC_MOVE * FOG_RECALC_MOVE
        || turn > FOG_RECALC_TURN
        || fog.since_recalc >= FOG_RECALC_PERIOD;

    if need_recalc {
        fog.since_recalc = 0.0;
        fog.last_pos = player;
        fog.last_aim = face;

        let fade2 = FOG_VIEW_FADE * FOG_VIEW_FADE;
        let near2 = FOG_NEAR_RADIUS * FOG_NEAR_RADIUS;
        let span = (FOG_VIEW_FADE - FOG_VIEW_FULL).max(1.0);
        // LOS целимся чуть «не доходя» до центра текселя, чтобы грани стен (их
        // центр за ближней гранью) считались видимыми, а не висели чёрными.
        let pull = TILE * 0.6;

        let fog = &mut *fog;
        let (cols, rows) = (fog.cols, fog.rows);
        let (map_w, map_h) = (fog.map_w, fog.map_h);
        for iy in 0..rows {
            for ix in 0..cols {
                let i = iy * cols + ix;
                let center = Vec2::new(
                    -map_w * 0.5 + (ix as f32 + 0.5) * TILE,
                    map_h * 0.5 - (iy as f32 + 0.5) * TILE,
                );
                let to = center - player;
                let dist2 = to.length_squared();

                // «Сейчас вижу» = внутри края обзора, В КОНУСЕ зрения (не за
                // спиной) И нет стены на линии взгляда. Дальние тайлы отсекаются
                // по дистанции ДО проверки конуса и LOS-рейкаста.
                let visible = dist2 <= fade2
                    && (dist2 <= near2
                        || protocol::combat::in_fov(
                            player,
                            face,
                            center,
                            protocol::constants::VIEW_FOV_HALF_ANGLE,
                        ))
                    && {
                        let dist = dist2.sqrt();
                        let test = if dist > pull {
                            center - to / dist * pull
                        } else {
                            player
                        };
                        !vision.0.segment_blocked(player, test, FOG_EPS)
                    };

                if visible {
                    fog.explored[i] = true;
                }
                fog.targets[i] = if visible {
                    // мягкий купол: в центре прозрачно, к краю обзора плавно
                    // гаснет до DARK_EXPLORED → чёткий «водораздел» вижу/не вижу
                    let dist = dist2.sqrt();
                    let t = ((dist - FOG_VIEW_FULL) / span).clamp(0.0, 1.0);
                    DARK_VISIBLE + t * (DARK_EXPLORED - DARK_VISIBLE)
                } else if fog.explored[i] {
                    DARK_EXPLORED
                } else {
                    DARK_HIDDEN
                };
            }
        }
    }

    // Каждый кадр: дешёвая релаксация darkness → targets (проход по f32-вектору).
    let smooth = 1.0 - (-FOG_SMOOTH_K * dt).exp();
    let fog = &mut *fog;
    for (d, &t) in fog.darkness.iter_mut().zip(fog.targets.iter()) {
        *d += (t - *d) * smooth;
    }
}

/// Туман-войны как ТИНТ: каждый кадр умножаем базовый цвет спрайта тайла (пол/
/// стены/пропы) на яркость = 1 − darkness в его тайле. В конусе — полная яркость,
/// вне — приглушённо. Меняем сами спрайты карты, поэтому эффект гарантированно
/// виден (в отличие от прежнего оверлея, который не композился).
pub fn apply_fog_tint(fog: Res<FogState>, mut q: Query<(&FogTint, &mut Sprite)>) {
    for (tint, mut sprite) in q.iter_mut() {
        let b = (1.0 - fog.darkness_at(tint.pos)).clamp(0.0, 1.0);
        // Квантуем яркость до шага 1/255 (мельче монитор всё равно не покажет):
        // без этого асимптотическое сглаживание давало микроскопически новое
        // значение каждый кадр, и ВСЕ спрайты карты помечались изменёнными —
        // рендер переизвлекал целую карту ежекадрово даже у стоящего игрока.
        // Это был главный пожиратель CPU на слабом железе.
        let b = (b * 255.0).round() / 255.0;
        let base = tint.base;
        let color = Color::srgba(base.red * b, base.green * b, base.blue * b, base.alpha);
        // Пишем в компонент ТОЛЬКО при реальном изменении (запись = флаг
        // change detection = переизвлечение спрайта рендером).
        if sprite.color != color {
            sprite.color = color;
        }
    }
}
