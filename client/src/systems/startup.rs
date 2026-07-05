use crate::resources::{ArrowTex, CircleTex, RingTex, UiFont};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Маркер игровой камеры (живёт только в `InGame`). Нужен, чтобы деспавнить её
/// при выходе из игры — иначе при возврате в меню/реконнекте остаётся «висеть»
/// вторая камера с тем же order, и рендер сыпет предупреждения об неоднозначности.
#[derive(Component)]
pub struct GameCamera;

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Tonemapping::None — без плёночной кривой 2D-спрайты показываются «как есть»
    // (по умолчанию тонмаппинг приглушает средние тона, из-за чего вся сцена
    // выглядела темновато). Яркость текстур дальше регулируем тинтом-множителем.
    commands.spawn((Camera2d, Tonemapping::None, GameCamera));

    // текстура круга для тела игрока (генерируем один раз)
    let handle = images.add(make_circle_image(64));
    commands.insert_resource(CircleTex(handle));

    // полый «ободок» (кольцо) для маркера под ногами игрока
    let ring = images.add(make_ring_image(96));
    commands.insert_resource(RingTex(ring));

    // стилизованная стрелка-указатель направления тела (вместо квадрата)
    let arrow = images.add(make_arrow_image(64));
    commands.insert_resource(ArrowTex(arrow));
}

/// Стилизованная стрелка-«дартик», смотрящая в +X (вправо). Форма — невыпуклый
/// четырёхугольник: остриё справа, два «крыла» сзади и вогнутая выемка по центру
/// задней кромки (классический силуэт стрелки). Заливка БЕЛАЯ (тонируется через
/// `Sprite.color`), вокруг — ТЁМНАЯ ОБВОДКА фиксированной толщины (остаётся
/// тёмной при любом тинте, поэтому стрелка всегда выделяется на фоне). Края
/// сглажены 3× суперсэмплингом.
fn make_arrow_image(size: u32) -> Image {
    // Вершины в нормализованных координатах [0,1] (u — вправо, v — вниз). Базовый
    // силуэт сдвинут внутрь, чтобы обводка уместилась в текстуру.
    let poly = [
        (0.90, 0.50), // остриё
        (0.12, 0.16), // верхнее крыло
        (0.42, 0.50), // выемка (вогнутость сзади)
        (0.12, 0.84), // нижнее крыло
    ];
    let border = 0.085_f32; // толщина обводки в норм. координатах

    // Точка внутри невыпуклого полигона (алгоритм лучей, even-odd).
    fn inside(poly: &[(f32, f32)], px: f32, py: f32) -> bool {
        let mut c = false;
        let n = poly.len();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
                c = !c;
            }
            j = i;
        }
        c
    }
    // Расстояние от точки до отрезка.
    fn dist_seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 1e-9 {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
    }
    // Мин. расстояние до контура полигона.
    fn dist_poly(poly: &[(f32, f32)], px: f32, py: f32) -> f32 {
        let n = poly.len();
        let mut d = f32::MAX;
        let mut j = n - 1;
        for i in 0..n {
            d = d.min(dist_seg(px, py, poly[j].0, poly[j].1, poly[i].0, poly[i].1));
            j = i;
        }
        d
    }

    let mut data = vec![0u8; (size * size * 4) as usize];
    let n = size as f32;
    const SS: usize = 3; // 3×3 суперсэмплинг
    let inv = 1.0 / (SS * SS) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut fill = 0.0f32; // доля заливки (белое)
            let mut brd = 0.0f32; // доля обводки (тёмное)
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n;
                    if inside(&poly, u, v) {
                        fill += inv;
                    } else if dist_poly(&poly, u, v) <= border {
                        brd += inv;
                    }
                }
            }
            let alpha = (fill + brd).clamp(0.0, 1.0);
            // цвет = доля белого среди покрытой площади (остальное — тёмная обводка)
            let white = if fill + brd > 1e-4 {
                fill / (fill + brd)
            } else {
                0.0
            };
            let c = (white * 255.0) as u8;
            let i = ((y * size + x) * 4) as usize;
            data[i] = c;
            data[i + 1] = c;
            data[i + 2] = c;
            data[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Белый круг с мягким краем (RGBA), тонируется через `Sprite.color`.
fn make_circle_image(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let r = size as f32 * 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            let d = (dx * dx + dy * dy).sqrt();
            let a = if d <= r - 1.0 {
                255.0
            } else if d <= r {
                255.0 * (r - d)
            } else {
                0.0
            };
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = a.clamp(0.0, 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Полый ободок (annulus) с мягкими краями: альфа только в кольце между внутренним
/// и внешним радиусом, центр прозрачный. Тонируется через `Sprite.color`.
fn make_ring_image(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let r = size as f32 * 0.5;
    let outer = r - 1.0; // внешний радиус
    let thickness = size as f32 * 0.16; // толщина ободка
    let inner = outer - thickness; // внутренний радиус
    let mid = (outer + inner) * 0.5;
    let halfw = (outer - inner) * 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            let d = (dx * dx + dy * dy).sqrt();
            // расстояние от средней линии кольца; мягкий спад на 1px по краям
            let edge = (halfw - (d - mid).abs() + 0.5).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (edge * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Деспавн игровой камеры при выходе из `InGame` (возврат в меню / реконнект),
/// чтобы не накапливались камеры с одинаковым order (рендер ругается на
/// неоднозначность и спамит предупреждения каждый кадр).
pub fn despawn_game_camera(mut commands: Commands, q: Query<Entity, With<GameCamera>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Полная зачистка сессионной сцены на выходе из игры (реконнект/меню): всё,
/// что помечено `SessionScoped` (уровень, HUD, подсветки, маркеры прицела,
/// трупы, декали), деспавнится разом. Без этого каждый реконнект спавнил
/// ВТОРЫЕ копии, и single()-системы молча отключались (контур удара замерзал
/// на полу, строки табла очков пропадали, кольцо гранаты отваливалось).
pub fn despawn_session_scene(
    mut commands: Commands,
    q: Query<Entity, With<crate::components::SessionScoped>>,
    mut corpses: ResMut<crate::resources::Corpses>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
    // Сущности трупов уже уходят по маркеру — чистим только реестр,
    // иначе Corpses.register после реконнекта деспавнил бы «призраков».
    corpses.0.clear();
}

pub fn load_ui_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.insert_resource(UiFont(handle));
}
