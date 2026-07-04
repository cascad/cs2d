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
    pub kick: Handle<Image>,
    pub hurt: Handle<Image>,
    pub cast: Handle<Image>,
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
            AnimState::Kick => self.kick.clone(),
            AnimState::Hurt => self.hurt.clone(),
            AnimState::Cast => self.cast.clone(),
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
        kick: asset_server.load("knight/Kick.png"),
        hurt: asset_server.load("knight/TakeDamage.png"),
        cast: asset_server.load("knight/CastSpell.png"),
        layout,
    });

    // --- пол: НАСТОЯЩИЕ тайлы kenney по материалу клетки (камень/земля/дерево/
    // плитка/щебень). Для «диабловской» неровности у материалов с несколькими
    // вариантами тайл выбирается детерминированным хэшем координат — рисунок пола
    // не повторяется монотонно, но одинаков у всех клиентов.
    // Под КЛЕТКАМИ-СТЕНАМИ пол НЕ рисуем: скала между комнатами — «пустота»
    // (тьма), а не гигантский пол с ободками, как раньше. ---
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
        if cell.wall.is_some() {
            continue;
        }
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
            crate::systems::fog::FogTint::new(tint, *center),
            Name::new("IsoFloor"),
        ));
    }
}

/// Тайлы-варианты пола для материала (имена файлов в `assets/iso_env/`).
/// ПОВТОРЕНИЕ имени = вес: ровные тайлы доминируют, «фактурные» (ямы, выбитые
/// плиты) — редкий акцент, иначе пол выглядит шумным месивом (выбор —
/// [`variant_index`]).
fn floor_variants(floor: Floor) -> &'static [&'static str] {
    match floor {
        Floor::Stone => &[
            "stone_N.png", "stone_N.png", "stone_N.png", "stone_N.png", "stone_N.png",
            "stoneUneven_N.png", "stoneMissingTiles_N.png",
        ],
        Floor::Dirt => &[
            "dirt_N.png", "dirt_N.png", "dirt_N.png", "dirt_N.png", "dirt_N.png",
            "dirtTiles_N.png",
        ],
        Floor::Wood => &[
            "planks_N.png", "planks_N.png", "planks_N.png", "planks_N.png", "planks_N.png",
            "planksBroken_N.png", "planksHole_N.png",
        ],
        Floor::Tiles => &[
            "stone_N.png", "stone_N.png", "stone_N.png", "stone_N.png", "stone_N.png",
            "stone_N.png", "stoneTile_N.png",
        ],
        Floor::Rubble => &[
            "stoneMissingTiles_N.png", "stoneMissingTiles_N.png", "stoneUneven_N.png",
        ],
    }
}

/// Детерминированный выбор варианта тайла по координатам клетки (одинаков у всех
/// клиентов). Финализатор в стиле murmur: простой линейный микс давал заметные
/// диагональные полосы одинаковых тайлов.
#[inline]
pub fn variant_index(ix: usize, iy: usize, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut h = (ix as u32).wrapping_mul(73_856_093) ^ (iy as u32).wrapping_mul(19_349_663);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
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

/// Порог сглаженной СКОРОСТИ (мир. ед./сек), выше которого актёр «идёт».
/// MOVE_SPEED=150, поэтому 25 надёжно отделяет ходьбу от сетевого шума.
const WALK_EPS_SPEED: f32 = 25.0;
/// Время сглаживания скорости (сек): убирает дребезг Walk↔Idle на кадрах, где
/// FixedUpdate не тикнул (FPS рендера не кратен тикрейту).
const MOVE_EMA_TAU: f32 = 0.1;

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
    let dt_s = time.delta_secs();
    for (wp, facing, mut anim, mut sprite) in q.iter_mut() {
        let step = (wp.0 - anim.prev).length();
        anim.prev = wp.0;
        if dt_s > 0.0 {
            let k = (dt_s / MOVE_EMA_TAU).clamp(0.0, 1.0);
            anim.move_ema = anim.move_ema * (1.0 - k) + (step / dt_s) * k;
        }
        let moving = anim.move_ema > WALK_EPS_SPEED;

        // оглушение: плавно тикаем остаток (снапшоты его перезаписывают). Пока
        // оглушён — модель замирает в Idle (ни ходьбы, ни действий); звёздочки над
        // головой рисует отдельная система по этому же `stun_left`.
        anim.stun_left = (anim.stun_left - dt.as_secs_f32()).max(0.0);
        let stunned = anim.stun_left > 0.0;
        if stunned && anim.state != AnimState::Idle {
            anim.state = AnimState::Idle;
            anim.frame = 0;
            anim.timer.reset();
            anim.lock_facing = None;
        }

        let oneshot = anim.state.is_oneshot();

        // нужная частота кадров зависит от типа анимации: клипы МЕХАНИЧЕСКИХ
        // действий растягиваются РОВНО на их геймплейное окно, чтобы визуал
        // заканчивался в тот же момент, когда возвращается управление/готов
        // следующий удар (единые окна из protocol::constants):
        // - рывок: 15 кадров ролла на DASH_DURATION;
        // - удар: 15 кадров взмаха на ATTACK_FACING_LOCK (= MELEE_SWING_TIME);
        // - удар щитом: 15 кадров Kick на STUN_FACING_LOCK (= STUN_SWING_TIME);
        // - косметические one-shot (Hurt/Cast) — ACTION_FPS; зацикленные — LOOP_FPS.
        let want_secs = match anim.state {
            AnimState::Dash => protocol::constants::DASH_DURATION / KNIGHT_COLS as f32,
            AnimState::Attack => protocol::constants::ATTACK_FACING_LOCK / KNIGHT_COLS as f32,
            AnimState::Kick => protocol::constants::STUN_FACING_LOCK / KNIGHT_COLS as f32,
            _ if oneshot => 1.0 / ACTION_FPS,
            _ => 1.0 / LOOP_FPS,
        };
        if (anim.timer.duration().as_secs_f32() - want_secs).abs() > 1e-4 {
            anim.timer
                .set_duration(std::time::Duration::from_secs_f32(want_secs));
        }

        if anim.timer.tick(dt).just_finished() {
            anim.frame = anim.frame.wrapping_add(1);
        }

        if stunned {
            // замерли в Idle — ничего не переключаем (кадр циклится сам)
        } else if oneshot {
            // доиграли действие — возвращаемся к idle/walk по факту движения
            if anim.frame >= KNIGHT_COLS {
                let base = if moving { AnimState::Walk } else { AnimState::Idle };
                anim.state = base;
                anim.frame = 0;
                anim.lock_facing = None;
            }
        } else {
            // блок перебивает idle/walk (держится, пока mouse2 установлен)
            let base = if anim.blocking {
                AnimState::Block
            } else if moving {
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
            continue; // блок/0 урона — без вспышки и без анимации боли
        }
        for (m, mut anim) in q.iter_mut() {
            if m.0 == ev.id {
                anim.hit_flash = HIT_FLASH_TIME;
                // Анимация получения урона (TakeDamage). НЕ прерываем уже идущие
                // одноразовые действия (удар/перекат/каст/удар щитом) и не мешаем
                // оглушённой позе — иначе ломали бы их визуал; играем только из
                // спокойного состояния (idle/walk/block).
                if !anim.state.is_oneshot() && anim.stun_left <= 0.0 {
                    anim.start_action(AnimState::Hurt);
                }
            }
        }
    }
}
