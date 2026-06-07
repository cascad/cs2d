use crate::resources::{CircleTex, MeleeMesh, UiFont};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use protocol::constants::MELEE_RANGE;

pub fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    commands.spawn(Camera2d::default());

    // текстура круга для тела игрока (генерируем один раз)
    let handle = images.add(make_circle_image(64));
    commands.insert_resource(CircleTex(handle));

    // узкое «лезвие» прочерка (радиус melee, маленький угол), один раз.
    // Сам прочерк создаётся вращением этого лезвия по конусу удара.
    let mesh = meshes.add(make_sector_mesh(MELEE_RANGE, BLADE_HALF_ANGLE, 6));
    commands.insert_resource(MeleeMesh(mesh));
}

/// Полу-угол узкого «лезвия» прочерка (рад). Сам клинок узкий, а ширину взмаха
/// задаёт амплитуда вращения (`MELEE_HALF_ANGLE`).
pub const BLADE_HALF_ANGLE: f32 = 0.22;

/// Заполненный сектор: вершина в начале координат, раскрытие ±`half_angle`
/// вокруг оси +X, радиус `radius`. Совпадает с зоной попадания удара.
fn make_sector_mesh(radius: f32, half_angle: f32, segments: u32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    let mut uvs: Vec<[f32; 2]> = vec![[0.5, 0.5]];
    for i in 0..=segments {
        let a = -half_angle + 2.0 * half_angle * (i as f32 / segments as f32);
        positions.push([radius * a.cos(), radius * a.sin(), 0.0]);
        uvs.push([0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin()]);
    }
    let mut indices: Vec<u32> = Vec::new();
    for i in 1..=segments {
        indices.push(0);
        indices.push(i);
        indices.push(i + 1);
    }
    let normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 1.0]; positions.len()];

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
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

pub fn load_ui_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.insert_resource(UiFont(handle));
}
