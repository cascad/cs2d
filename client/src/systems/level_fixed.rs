use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use protocol::constants::TILE_SIZE;
use protocol::geom::WallGrid;
use protocol::maps::{self, Prop};

use crate::{
    render::{depth_z, layers, world_to_screen, RenderLayer, WorldPos},
    resources::{SolidTiles, SpawnPoints, VisionGridRes, WallAabbCache, WallGridRes},
    systems::iso::{variant_index, TILE_ANCHOR_Y},
    systems::level::Wall,
};

pub const TILE: f32 = 32.0;

/// Размеры активной карты (в тайлах): (ширина, высота). Источник — общая
/// `protocol::maps`, чтобы туман и границы камеры совпадали с геометрией.
pub fn map_dims() -> (usize, usize) {
    let lvl = maps::active_level(TILE);
    (lvl.width, lvl.height)
}

/// Общий множитель яркости стен/пропов (тайл-ассет тёмный — осветляем тинтом).
/// Держим в тон полу (`FLOOR_BRIGHTNESS` в `iso.rs`).
const WALL_BRIGHTNESS: f32 = 1.15;

/// Загрузчик тайлов окружения с линейным сэмплингом (как у пола) — без «лесенок»
/// при масштабировании. Кэширует хэндлы по имени файла.
struct EnvLoader<'a> {
    server: &'a AssetServer,
    cache: std::collections::HashMap<&'static str, Handle<Image>>,
}
impl<'a> EnvLoader<'a> {
    fn new(server: &'a AssetServer) -> Self {
        Self { server, cache: Default::default() }
    }
    fn get(&mut self, name: &'static str) -> Handle<Image> {
        let server = self.server;
        self.cache
            .entry(name)
            .or_insert_with(|| {
                server.load_with_settings(
                    format!("iso_env/{name}"),
                    |s: &mut ImageLoaderSettings| {
                        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                    },
                )
            })
            .clone()
    }
}

/// Файл-спрайт пропа.
fn prop_image(prop: Prop) -> &'static str {
    match prop {
        Prop::Barrel => "barrel_E.png",
        Prop::Barrels => "barrels_E.png",
        Prop::BarrelsStacked => "barrelsStacked_E.png",
        Prop::Chest => "chestClosed_E.png",
        Prop::Column => "stoneColumn_E.png",
    }
}

/// Построение уровня из ОБЩЕЙ карты (`protocol::maps`): считает коллизию (мир) и
/// спавнит изо-визуал стен и пропов из пред-рендеренных тайлов kenney.
/// Возвращает (SolidTiles, спавны, AABB стен).
pub fn create_fixed_level(
    commands: &mut Commands,
    asset_server: &AssetServer,
) -> (SolidTiles, Vec<Vec2>, Vec<(Vec2, Vec2)>, Vec<(Vec2, Vec2)>) {
    let lvl = maps::active_level(TILE);
    let (w, h) = (lvl.width, lvl.height);
    let mut env = EnvLoader::new(asset_server);

    // решётка сплошных КЛЕТОК-СТЕН (из символьной карты; пропы сюда не входят —
    // они на полу). Нужна, чтобы НЕ рисовать «погребённые» стены внутри толстых
    // массивов (все 4 соседа — стены): их всё равно не видно, а отрисовка тяжелее.
    let wall_solid: Vec<bool> = lvl.cells.iter().map(|(_, c)| c.wall.is_some()).collect();
    let is_wall = |ix: i32, iy: i32| -> bool {
        if ix < 0 || iy < 0 || ix >= w as i32 || iy >= h as i32 {
            return true; // вне карты считаем «стеной»
        }
        wall_solid[iy as usize * w + ix as usize]
    };

    // Стену рисуем выше спрайта, чтобы блок поднимался над полом. Якорь — доля
    // СОДЕРЖИМОГО (основание ромба), поэтому при растяжении вертикали основание
    // остаётся на world_to_screen(center), а верх тянется выше.
    const WALL_HEIGHT_SCALE: f32 = 1.6;
    let wall_size = Vec2::new(TILE * 2.0, TILE * 4.0 * WALL_HEIGHT_SCALE);
    let anchor = Anchor(Vec2::new(0.0, TILE_ANCHOR_Y));
    let tint = Color::srgb(WALL_BRIGHTNESS, WALL_BRIGHTNESS, WALL_BRIGHTNESS);

    // --- стены: ПОЛНЫЙ блок (обе грани, обращённые к камере) — у блока ровный
    // верх (топ-кап входит в спрайт), поэтому ряды стыкуются в сплошную стену без
    // «зубцов». Рисуем только КРОМКУ массива (тайл с хотя бы одним соседом-полом):
    // внутренние тайлы толстых стен невидимы. Часть стен «состаренные» (вариатив-
    // ность, детерминированно по xy). Правая грань ниже, левая чуть поверх — чтобы
    // по общему переднему ребру не было z-fight. ---
    for (i, (center, cell)) in lvl.cells.iter().enumerate() {
        if cell.wall.is_none() {
            continue;
        }
        let (ix, iy) = ((i % w) as i32, (i / w) as i32);
        // Пропускаем стену ТОЛЬКО если она замурована со ВСЕХ 8 сторон (включая
        // диагонали): иначе у вогнутых углов (косяки проёмов, углы комнат) стена
        // видна/задевается по диагонали, но не рисовалась бы — «коллизия есть,
        // стены нет».
        let buried = [
            (-1, -1), (0, -1), (1, -1),
            (-1, 0), (1, 0),
            (-1, 1), (0, 1), (1, 1),
        ]
        .iter()
        .all(|&(dx, dy)| is_wall(ix + dx, iy + dy));
        if buried {
            continue;
        }
        let aged = variant_index(ix as usize, iy as usize, 4) == 0; // ~25% «состаренных»
        let (n_name, w_name) = if aged {
            ("stoneWallAged_N.png", "stoneWallAged_W.png")
        } else {
            ("stoneWall_N.png", "stoneWall_W.png")
        };
        let s = world_to_screen(*center);
        for (name, layer) in [(w_name, layers::WALL), (n_name, layers::WALL + 0.01)] {
            commands.spawn((
                Sprite { image: env.get(name), color: tint, custom_size: Some(wall_size), ..default() },
                anchor,
                Transform::from_xyz(s.x, s.y, depth_z(*center, layer)),
                GlobalTransform::default(),
                WorldPos(*center),
                RenderLayer(layer),
                crate::systems::fog::FogTint::new(tint, *center),
                Wall,
            ));
        }
    }

    // --- пропы: бочки/сундуки/колонны. Слой зависит от того, ПРОСВЕЧИВАЕТ ли проп
    // (тот же признак, что и для тумана — `blocks_vision`):
    //  • глухие (колонны) — в слое АКТЁРОВ с y-сортировкой: живой/труп корректно
    //    перекрываются по глубине, а ТЕЛО остаётся ЗА колонной (она выше трупов);
    //  • просвечивающие (бочки/сундуки) — в отдельном НИЗКОМ слое под трупами,
    //    чтобы лежащее тело рисовалось ПОВЕРХ них (их низкий силуэт не должен
    //    прятать труп). Коллизия одинаковая (она в lvl.wall_aabbs). ---
    let prop_size = Vec2::new(TILE * 2.0, TILE * 4.0);
    for (center, prop) in &lvl.props {
        let layer = if prop.blocks_vision() {
            layers::ACTOR
        } else {
            layers::PROP_SEETHROUGH
        };
        let s = world_to_screen(*center);
        commands.spawn((
            Sprite {
                image: env.get(prop_image(*prop)),
                color: tint,
                custom_size: Some(prop_size),
                ..default()
            },
            anchor,
            Transform::from_xyz(s.x, s.y, depth_z(*center, layer)),
            GlobalTransform::default(),
            WorldPos(*center),
            RenderLayer(layer),
            crate::systems::fog::FogTint::new(tint, *center),
        ));
    }

    (SolidTiles(lvl.solid_tiles()), lvl.spawns, lvl.wall_aabbs, lvl.vision_aabbs)
}

/// Системный сетап: один раз строим уровень и кладём ресурсы. Спатиал-сетка
/// стен (`WallGrid`) и кэш AABB строятся СРАЗУ из мировых данных.
pub fn setup_fixed_level(mut commands: Commands, asset_server: Res<AssetServer>) {
    let (solid, spawns, wall_aabbs, vision_aabbs) =
        create_fixed_level(&mut commands, &asset_server);

    // Сетка движения — все стены/пропы; сетка взгляда (туман) — без низких пропов.
    let grid = WallGrid::build(&wall_aabbs, TILE_SIZE * 2.0);
    let vision_grid = WallGrid::build(&vision_aabbs, TILE_SIZE * 2.0);
    commands.insert_resource(solid);
    commands.insert_resource(SpawnPoints(spawns));
    commands.insert_resource(WallGridRes(grid));
    commands.insert_resource(VisionGridRes(vision_grid));
    commands.insert_resource(WallAabbCache(wall_aabbs));
}
