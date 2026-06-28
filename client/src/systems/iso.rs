//! Изометрия (ala Diablo): рыцарь (атлас 8 направлений × N кадров), пол из
//! пред-рендеренных тайлов kenney и система покадровой анимации актёров.
//!
//! Мир остаётся плоскостью X/Y; здесь только ОТРИСОВКА. Строка спрайт-листа
//! (1 из 8 направлений) выбирается из ЭКРАННОЙ проекции угла [`Facing`] — так
//! рыцарь смотрит туда же, куда курсор на экране, независимо от изо-поворота сцены.

use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::components::{ActorAnim, AnimState};
use crate::render::{depth_z, layers, world_to_screen, WorldPos};
use crate::systems::level_fixed::TILE;
use protocol::maps::{self, Floor};

// --- параметры листа рыцаря (2D HD Character Knight) ---------------------------

/// Кадров в ряду листа рыцаря (по 1 ряду на направление).
pub const KNIGHT_COLS: usize = 15;
/// Рядов = направлений на листе.
pub const KNIGHT_ROWS: usize = 8;
/// Размер одной клетки листа (px), листы 1920×1024 = 15×8 по 128.
pub const KNIGHT_CELL: u32 = 128;

/// Экранный размер квадрата спрайта рыцаря (px). Клетка 128×128, рыцарь занимает
/// её большую часть. Тюнится в полише.
pub const ISO_ACTOR_PX: f32 = 150.0;

/// Якорь рыцаря по Y (доля от центра, +вверх): сдвигаем «ноги» к `WorldPos`, чтобы
/// кольцо-маркер было ровно под ногами и персонаж стоял на тайле.
pub const KNIGHT_ANCHOR_Y: f32 = -0.30;

/// FPS зацикленных idle/walk и одноразовых действий (атака/рывок).
const LOOP_FPS: f32 = 12.0;
const ACTION_FPS: f32 = 24.0;

/// Хэндлы листов рыцаря + общий layout атласа. Все листы — одна сетка 15×8.
#[derive(Resource, Clone)]
pub struct KnightAnims {
    pub idle: Handle<Image>,
    pub walk: Handle<Image>,
    pub melee: Handle<Image>,
    pub rolling: Handle<Image>,
    pub block: Handle<Image>,
    pub death: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
}

impl KnightAnims {
    /// Лист для состояния анимации.
    pub fn sheet(&self, state: AnimState) -> Handle<Image> {
        match state {
            AnimState::Idle => self.idle.clone(),
            AnimState::Walk => self.walk.clone(),
            AnimState::Attack => self.melee.clone(),
            AnimState::Dash => self.rolling.clone(),
            AnimState::Block => self.block.clone(),
        }
    }
}

/// Доводка соответствия «направление взгляда → ряд листа» в шагах по 45°.
/// Если «перёд» рыцаря смотрит не туда — измени это ОДНО число (±1 = поворот
/// спрайта на 45°). 0 — выверено по листам Walk/Idle (row6=Юг/лицом к камере).
pub const KNIGHT_ROW_OFFSET: i32 = 0;

/// Строка листа (0..7) из МИРОВОГО угла взгляда: проецируем направление в экран
/// (проекция линейна через 0,0) и берём 8-сектор экранного угла.
///
/// Ряды листа (выверено по обратной связи): экранный угол ряда = 360° − 45·row,
/// откуда базово row = (8 − idx8) mod 8 (т.е. row0=Юг/лицом к камере, дальше
/// против часовой). Точную доводку см. `KNIGHT_ROW_OFFSET`.
#[inline]
pub fn knight_row(facing_world: f32) -> usize {
    let world_dir = Vec2::new(facing_world.cos(), facing_world.sin());
    let s = world_to_screen(world_dir);
    let deg = s.y.atan2(s.x).to_degrees().rem_euclid(360.0);
    // 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE (экран, +y вверх)
    let idx8 = ((deg / 45.0).round() as i32).rem_euclid(8);
    (8 - idx8 + KNIGHT_ROW_OFFSET).rem_euclid(8) as usize
}

/// Загрузка листов рыцаря + пол из тайлов kenney. Один раз на старте.
pub fn setup_iso(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    // --- рыцарь: 4 листа одной сетки 15×8, общий layout атласа ---
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::splat(KNIGHT_CELL),
        KNIGHT_COLS as u32,
        KNIGHT_ROWS as u32,
        None,
        None,
    ));
    commands.insert_resource(KnightAnims {
        idle: asset_server.load("knight/Idle.png"),
        walk: asset_server.load("knight/Walk.png"),
        melee: asset_server.load("knight/Melee.png"),
        rolling: asset_server.load("knight/Rolling.png"),
        block: asset_server.load("knight/Block.png"),
        death: asset_server.load("knight/Death.png"),
        layout,
    });

    // --- пол: НАСТОЯЩИЕ тайлы kenney по материалу клетки (камень/земля/дерево/
    // плитка/щебень). Для «диабловской» неровности у материалов с несколькими
    // вариантами тайл выбирается детерминированным хэшем координат — рисунок пола
    // не повторяется монотонно, но одинаков у всех клиентов. ---
    let lvl = maps::active_level(TILE);

    // загрузка картинок пола один раз (asset_server дедуплицирует по пути).
    let mut cache: std::collections::HashMap<&'static str, Handle<Image>> = Default::default();
    let mut load = |name: &'static str| -> Handle<Image> {
        cache
            .entry(name)
            .or_insert_with(|| {
                asset_server.load_with_settings(
                    format!("iso_env/{name}"),
                    |s: &mut ImageLoaderSettings| {
                        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                    },
                )
            })
            .clone()
    };

    // натуральный тайл kenney 256×512 → экранный ромб одной клетки (TILE*2 × TILE*4)
    let tile_size = Vec2::new(TILE * 2.0, TILE * 4.0);
    let tile_anchor = Anchor(Vec2::new(0.0, TILE_ANCHOR_Y));
    let tint = Color::srgb(FLOOR_BRIGHTNESS, FLOOR_BRIGHTNESS, FLOOR_BRIGHTNESS);

    for (i, (center, cell)) in lvl.cells.iter().enumerate() {
        let Some(floor) = cell.floor else { continue };
        let (ix, iy) = (i % lvl.width, i / lvl.width);
        let variants = floor_variants(floor);
        let name = variants[variant_index(ix, iy, variants.len())];
        let s = world_to_screen(*center);
        commands.spawn((
            Sprite {
                image: load(name),
                color: tint,
                custom_size: Some(tile_size),
                ..default()
            },
            tile_anchor,
            Transform::from_xyz(s.x, s.y, depth_z(*center, layers::FLOOR)),
            Name::new("IsoFloor"),
        ));
    }
}

/// Тайлы-варианты пола для материала (имена файлов в `assets/iso_env/`). Несколько
/// вариантов → пол не выглядит монотонной плиткой (выбор — [`variant_index`]).
fn floor_variants(floor: Floor) -> &'static [&'static str] {
    match floor {
        Floor::Stone => &["stone_N.png", "stoneTile_N.png", "stoneUneven_N.png"],
        Floor::Dirt => &["dirt_N.png", "dirtTiles_N.png"],
        Floor::Wood => &["planks_N.png"],
        Floor::Tiles => &["stoneTile_N.png"],
        Floor::Rubble => &["stoneMissingTiles_N.png", "stoneUneven_N.png"],
    }
}

/// Детерминированный выбор варианта тайла по координатам клетки (одинаков у всех
/// клиентов). Простой хэш-микс, чтобы соседние клетки не совпадали.
#[inline]
pub fn variant_index(ix: usize, iy: usize, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let h = (ix as u32).wrapping_mul(73_856_093) ^ (iy as u32).wrapping_mul(19_349_663);
    (h % n as u32) as usize
}

/// Общий множитель яркости пола: тайл-ассет сам по себе тёмный, поэтому
/// осветляем тинтом (Sprite.color работает как множитель). Подними/опусти это
/// одно число, чтобы сделать карту светлее/темнее. Держим в тон стенам
/// (`WALL_BRIGHTNESS`) и персонажам (`ACTOR_BRIGHTNESS`), чтобы яркость была
/// равномерной и ничего не пересвечивало.
const FLOOR_BRIGHTNESS: f32 = 1.15;

/// Множитель яркости моделей игроков (рыцарь). Тинтуется каждый кадр в
/// `animate_actors`; держим заодно с полом/стенами, чтобы персонажи не были
/// тёмными силуэтами на фоне светлой карты.
pub const ACTOR_BRIGHTNESS: f32 = 1.15;

/// Якорь тайлов kenney (пол/стены) по Y: точка на ромбе-основании, совмещаемая с
/// `world_to_screen(центр клетки)`. Подобрано под лист 256×512 (верх ромба ~y364).
pub const TILE_ANCHOR_Y: f32 = -0.336;

/// Порог перемещения за кадр (мировые ед.), выше которого считаем актёра идущим.
const WALK_EPS: f32 = 0.35;

/// Покадровая анимация направленных актёров (рыцарь): выбирает строку листа из
/// [`Facing`], состояние idle/walk — по факту перемещения `WorldPos`; одноразовые
/// `Attack`/`Dash` проигрывает один раз и возвращает к idle/walk. Пишет кадр в
/// `Sprite` (лист + индекс атласа).
pub fn animate_actors(
    time: Res<Time>,
    anims: Res<KnightAnims>,
    mut q: Query<(&WorldPos, &crate::components::Facing, &mut ActorAnim, &mut Sprite)>,
) {
    let dt = time.delta();
    for (wp, facing, mut anim, mut sprite) in q.iter_mut() {
        let moved = (wp.0 - anim.prev).length();
        anim.prev = wp.0;

        let oneshot = anim.state.is_oneshot();

        // нужная частота кадров зависит от типа анимации:
        // - рывок: 15 кадров ролла растягиваем РОВНО на длительность рывка
        //   (DASH_DURATION), чтобы анимация заканчивалась тогда же, когда снимается
        //   блокировка управления — «можно двигаться только после конца рывка»;
        // - прочие одноразовые (удар) — ACTION_FPS; зацикленные — LOOP_FPS.
        let want_secs = if anim.state == AnimState::Dash {
            protocol::constants::DASH_DURATION / KNIGHT_COLS as f32
        } else if oneshot {
            1.0 / ACTION_FPS
        } else {
            1.0 / LOOP_FPS
        };
        if (anim.timer.duration().as_secs_f32() - want_secs).abs() > 1e-4 {
            anim.timer
                .set_duration(std::time::Duration::from_secs_f32(want_secs));
        }

        if anim.timer.tick(dt).just_finished() {
            anim.frame = anim.frame.wrapping_add(1);
        }

        if oneshot {
            // доиграли действие — возвращаемся к idle/walk по факту движения
            if anim.frame >= KNIGHT_COLS {
                let base = if moved > WALK_EPS { AnimState::Walk } else { AnimState::Idle };
                anim.state = base;
                anim.frame = 0;
                anim.lock_facing = None;
            }
        } else {
            // блок перебивает idle/walk (держится, пока mouse2 установлен)
            let base = if anim.blocking {
                AnimState::Block
            } else if moved > WALK_EPS {
                AnimState::Walk
            } else {
                AnimState::Idle
            };
            if base != anim.state {
                anim.state = base;
                anim.frame = 0;
                anim.timer.reset();
            }
        }

        // во время рывка спрайт смотрит туда, куда катимся (lock_facing), иначе — на курсор
        let dir_angle = anim.lock_facing.unwrap_or(facing.0);
        let row = knight_row(dir_angle);
        let index = row * KNIGHT_COLS + (anim.frame % KNIGHT_COLS);
        sprite.image = anims.sheet(anim.state);
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.layout = anims.layout.clone();
            atlas.index = index;
        } else {
            sprite.texture_atlas = Some(TextureAtlas {
                layout: anims.layout.clone(),
                index,
            });
        }

        // «вспышка урона»: краснеем при получении урона и плавно гаснем обратно.
        // Меняем только RGB, сохраняя альфу (её ведёт fade_unseen_players).
        let a = sprite.color.alpha();
        let b = ACTOR_BRIGHTNESS;
        if anim.hit_flash > 0.0 {
            anim.hit_flash = (anim.hit_flash - dt.as_secs_f32()).max(0.0);
            let k = (anim.hit_flash / HIT_FLASH_TIME).clamp(0.0, 1.0); // 1→0
            sprite.color = Color::srgba(b, b * (1.0 - 0.85 * k), b * (1.0 - 0.85 * k), a);
        } else {
            sprite.color = Color::srgba(b, b, b, a);
        }
    }
}

/// Длительность «вспышки урона» (сек): модель краснеет и возвращается к норме.
pub const HIT_FLASH_TIME: f32 = 0.32;

/// Зажигает «вспышку урона» на модели игрока, получившего урон (>0). Срабатывает
/// и для локального, и для удалённых игроков — видно, что по кому-то прошёл удар.
pub fn flash_on_damage(
    mut reader: MessageReader<crate::events::PlayerDamagedEvent>,
    mut q: Query<(&crate::components::PlayerMarker, &mut ActorAnim)>,
) {
    for ev in reader.read() {
        if ev.damage <= 0 {
            continue;
        }
        for (m, mut anim) in q.iter_mut() {
            if m.0 == ev.id {
                anim.hit_flash = HIT_FLASH_TIME;
            }
        }
    }
}
