use crate::resources::{CircleTex, RingTex, UiFont};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d::default());

    // текстура круга для тела игрока (генерируем один раз)
    let handle = images.add(make_circle_image(64));
    commands.insert_resource(CircleTex(handle));

    // полый «ободок» (кольцо) для маркера под ногами игрока
    let ring = images.add(make_ring_image(96));
    commands.insert_resource(RingTex(ring));
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

pub fn load_ui_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.insert_resource(UiFont(handle));
}
