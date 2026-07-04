//! Офлайн-превью карты: композитит активный уровень в PNG той же изометрией,
//! что клиентский рендер (2:1, тайл 64×32 экранных px при TILE=32).
//!
//! Нужен, чтобы итерироваться по автотайлингу стен БЕЗ запуска игры:
//!     cargo run -p protocol --example map_preview
//! Спрайты берутся из пака kenney (переопределяется env `KENNEY_ISO_DIR`),
//! результат — `map_preview.png` в корне репо.

use glam::Vec2;
use image::{imageops, DynamicImage, RgbaImage};
use protocol::maps::{self, Floor, Prop};
use std::collections::HashMap;

const TILE: f32 = 32.0;
const ISO_X: f32 = 1.0;
const ISO_Y: f32 = 0.5;

/// Масштаб спрайтов пака (256×512 → 64×128, как в игре: custom_size TILE*2×TILE*4).
const SCALE: u32 = 4; // делитель
const SPR_W: u32 = 256 / SCALE;
const SPR_H: u32 = 512 / SCALE;
/// Якорь тайла в канвасе спрайта (центр ромба-основания): (128, 428) в 256×512.
const ANCHOR_X: i64 = 128 / SCALE as i64;
const ANCHOR_Y: i64 = 428 / SCALE as i64;

fn world_to_screen(w: Vec2) -> Vec2 {
    Vec2::new((w.x - w.y) * ISO_X, (w.x + w.y) * ISO_Y)
}

/// Кэш загруженных и отмасштабированных спрайтов пака.
struct Sprites {
    dir: String,
    cache: HashMap<String, RgbaImage>,
}
impl Sprites {
    fn get(&mut self, name: &str) -> &RgbaImage {
        if !self.cache.contains_key(name) {
            let path = format!("{}/{}.png", self.dir, name);
            let img = image::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let img = imageops::resize(
                &img.to_rgba8(),
                SPR_W,
                SPR_H,
                imageops::FilterType::CatmullRom,
            );
            self.cache.insert(name.to_string(), img);
        }
        &self.cache[name]
    }
}

/// Варианты пола: повторение имени = вес (ровные тайлы должны доминировать,
/// «фактурные» — редкий акцент, иначе пол выглядит шумным месивом).
fn floor_variants(floor: Floor) -> &'static [&'static str] {
    match floor {
        Floor::Stone => &["stone_N", "stone_N", "stone_N", "stone_N", "stone_N", "stoneUneven_N", "stoneMissingTiles_N"],
        Floor::Dirt => &["dirt_N", "dirt_N", "dirt_N", "dirt_N", "dirt_N", "dirtTiles_N"],
        Floor::Wood => &["planks_N", "planks_N", "planks_N", "planks_N", "planks_N", "planksBroken_N", "planksHole_N"],
        Floor::Tiles => &["stone_N", "stone_N", "stone_N", "stone_N", "stone_N", "stone_N", "stoneTile_N"],
        Floor::Rubble => &["stoneMissingTiles_N", "stoneMissingTiles_N", "stoneUneven_N"],
    }
}

fn prop_image(prop: Prop) -> &'static str {
    match prop {
        Prop::Barrel => "barrel_E",
        Prop::Barrels => "barrels_E",
        Prop::BarrelsStacked => "barrelsStacked_E",
        Prop::Chest => "chestClosed_E",
        Prop::Column => "stoneColumn_E",
    }
}

fn variant_index(ix: usize, iy: usize, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    // финализатор в стиле murmur: без него линейный микс даёт диагональные полосы
    let mut h = (ix as u32).wrapping_mul(73_856_093) ^ (iy as u32).wrapping_mul(19_349_663);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h % n as u32) as usize
}

fn main() {
    let dir = std::env::var("KENNEY_ISO_DIR").unwrap_or_else(|_| {
        "F:/diablo_assets/kenney_isometric-miniature-dungeon/Isometric".to_string()
    });
    let mut sprites = Sprites { dir, cache: HashMap::new() };

    let lvl = maps::active_level(TILE);
    let (w, h) = (lvl.width, lvl.height);
    let wall_solid: Vec<bool> = lvl.cells.iter().map(|(_, c)| c.wall.is_some()).collect();
    let is_wall = |ix: i32, iy: i32| -> bool {
        if ix < 0 || iy < 0 || ix >= w as i32 || iy >= h as i32 {
            return true;
        }
        wall_solid[iy as usize * w + ix as usize]
    };

    // --- границы изображения ---
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (c, _) in &lvl.cells {
        let s = world_to_screen(*c);
        min_x = min_x.min(s.x);
        max_x = max_x.max(s.x);
        min_y = min_y.min(s.y);
        max_y = max_y.max(s.y);
    }
    let margin = 80.0;
    let img_w = (max_x - min_x + margin * 2.0) as u32;
    let img_h = (max_y - min_y + margin * 2.0) as u32;
    let mut img = RgbaImage::from_pixel(img_w, img_h, image::Rgba([12, 10, 14, 255]));

    // экранная точка -> пиксель картинки (y растёт вниз)
    let to_px = |s: Vec2| -> (i64, i64) {
        (
            (s.x - min_x + margin) as i64,
            (max_y - s.y + margin) as i64,
        )
    };

    let paste = |img: &mut RgbaImage, spr: &RgbaImage, center: Vec2| {
        let (px, py) = to_px(world_to_screen(center));
        imageops::overlay(img, spr, px - ANCHOR_X, py - ANCHOR_Y);
    };

    // Стены слегка вытягиваем по вертикали (как в игре, но умеренно): основание
    // остаётся на месте (низ канваса совпадает), верх поднимается.
    const WALL_STRETCH: f32 = 1.35;
    let paste_wall = |img: &mut RgbaImage, spr: &RgbaImage, center: Vec2| {
        let hh = (SPR_H as f32 * WALL_STRETCH) as u32;
        let stretched = imageops::resize(spr, SPR_W, hh, imageops::FilterType::CatmullRom);
        let (px, py) = to_px(world_to_screen(center));
        // якорь по Y масштабируется вместе с высотой относительно НИЗА канваса:
        // низ спрайта остаётся на том же экранном месте.
        let ay = hh as i64 - (SPR_H as i64 - ANCHOR_Y);
        imageops::overlay(img, &stretched, px - ANCHOR_X, py - ay);
    };

    // --- 1) пол: только там, где НЕТ стены ---
    for (i, (center, cell)) in lvl.cells.iter().enumerate() {
        if cell.wall.is_some() {
            continue;
        }
        let Some(floor) = cell.floor else { continue };
        let (ix, iy) = (i % w, i / w);
        let variants = floor_variants(floor);
        let name = variants[variant_index(ix, iy, variants.len())];
        let spr = sprites.get(name).clone();
        paste(&mut img, &spr, *center);
    }

    // --- 2) стены и пропы: painter's algorithm, дальние (x+y больше) раньше ---
    struct Item {
        center: Vec2,
        names: Vec<&'static str>,
        wall: bool,
    }
    let mut items: Vec<Item> = Vec::new();

    for (i, (center, cell)) in lvl.cells.iter().enumerate() {
        if cell.wall.is_none() {
            continue;
        }
        let (ix, iy) = ((i % w) as i32, (i / w) as i32);
        // кромки, за которыми пол (открытые грани). Порядок отрисовки в клетке:
        // сперва дальние (NW=+y, NE=+x), потом ближние (SW=-x, SE=-y).
        let aged = variant_index(ix as usize, iy as usize, 4) == 0;
        let mut names: Vec<&'static str> = Vec::new();
        let pick = |plain: &'static str, aged_n: &'static str| if aged { aged_n } else { plain };
        if !is_wall(ix, iy + 1) {
            names.push(pick("stoneWall_E", "stoneWallAged_E")); // NW кромка (+y)
        }
        if !is_wall(ix + 1, iy) {
            names.push(pick("stoneWall_S", "stoneWallAged_S")); // NE кромка (+x)
        }
        if !is_wall(ix - 1, iy) {
            names.push(pick("stoneWall_N", "stoneWallAged_N")); // SW кромка (-x)
        }
        if !is_wall(ix, iy - 1) {
            names.push(pick("stoneWall_W", "stoneWallAged_W")); // SE кромка (-y)
        }
        if names.is_empty() {
            continue;
        }
        items.push(Item { center: *center, names, wall: true });
    }
    for (center, prop) in &lvl.props {
        items.push(Item { center: *center, names: vec![prop_image(*prop)], wall: false });
    }

    // дальние раньше
    items.sort_by(|a, b| {
        let ka = a.center.x + a.center.y;
        let kb = b.center.x + b.center.y;
        kb.partial_cmp(&ka).unwrap()
    });
    for it in &items {
        for name in &it.names {
            let spr = sprites.get(name).clone();
            if it.wall {
                paste_wall(&mut img, &spr, it.center);
            } else {
                paste(&mut img, &spr, it.center);
            }
        }
    }

    // --- 3) рыцарь для масштаба (кадр Idle, ряд 6 = лицом к камере) ---
    if let Ok(knight) = image::open("assets/knight/Idle.png") {
        let frame = knight.crop_imm(0, 6 * 128, 128, 128);
        let frame = imageops::resize(&frame.to_rgba8(), 150, 150, imageops::FilterType::CatmullRom);
        // центр комнаты C (26, 20) в тайлах
        let center = lvl.tile_center(26, 20);
        let (px, py) = to_px(world_to_screen(center));
        imageops::overlay(&mut img, &frame, px - 75, py - 120);
    }

    let out = "map_preview.png";
    DynamicImage::ImageRgba8(img).save(out).unwrap();
    println!("saved {out}");
}
