//! Миникарта (toggle по `M`) в левом верхнем углу: показывает геометрию уровня
//! (стены/пол/переходы), прокручивается так, чтобы локальный игрок всегда был в
//! центре окна. Противников НЕ отображаем — только маркер себя по центру.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::components::LocalPlayer;
use crate::render::WorldPos;
use crate::systems::level_fixed::TILE;
use protocol::maps;

/// Сколько экранных px занимает один тайл карты на миникарте.
const MM_SCALE: f32 = 4.0;
/// Размер окна миникарты (px).
const MM_VIEW: f32 = 200.0;

/// Геометрия карты для пересчёта мир→миникарта.
#[derive(Resource)]
pub struct MinimapMeta {
    origin: Vec2,
    tile: f32,
    h: usize,
}

/// Корневой контейнер миникарты (его `Visibility` переключаем по `M`).
#[derive(Component)]
pub struct MinimapRoot;

/// Прокручиваемое изображение карты внутри окна.
#[derive(Component)]
pub struct MinimapImage;

/// Строит текстуру карты и UI-узлы миникарты.
pub fn setup_minimap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let lvl = maps::active_level(TILE);
    let (w, h) = (lvl.width, lvl.height);

    // 1 тексель на тайл; ось Y разворачиваем (север — вверху).
    let mut data = vec![0u8; w * h * 4];
    for (i, (_, cell)) in lvl.cells.iter().enumerate() {
        let jx = i % w;
        let jy = i / w;
        let ty = (h - 1) - jy;
        let idx = (ty * w + jx) * 4;
        let rgba: [u8; 4] = if cell.solid {
            [58, 62, 80, 255] // стена — тёмно-синий камень
        } else if cell.floor.is_some() {
            [184, 176, 152, 255] // пол/коридор/переход — светлый
        } else {
            [0, 0, 0, 0] // пустота вне уровня — прозрачно
        };
        data[idx..idx + 4].copy_from_slice(&rgba);
    }

    let mut image = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::nearest(); // чёткие пиксели миникарты
    let handle = images.add(image);

    commands.insert_resource(MinimapMeta { origin: lvl.origin, tile: lvl.tile, h });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                width: Val::Px(MM_VIEW),
                height: Val::Px(MM_VIEW),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.06, 0.85)),
            GlobalZIndex(50),
            MinimapRoot,
            crate::components::SessionScoped,
            Visibility::Visible,
            Name::new("Minimap"),
        ))
        .with_children(|p| {
            p.spawn((
                ImageNode::new(handle.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(w as f32 * MM_SCALE),
                    height: Val::Px(h as f32 * MM_SCALE),
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    ..default()
                },
                MinimapImage,
            ));
            // маркер «я» — всегда в центре окна (карта прокручивается под ним)
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(MM_VIEW * 0.5 - 3.0),
                    top: Val::Px(MM_VIEW * 0.5 - 3.0),
                    width: Val::Px(6.0),
                    height: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.3, 1.0, 0.5)),
                GlobalZIndex(51),
            ));
        });
}

/// Прокрутка миникарты: держим локального игрока в центре окна.
pub fn update_minimap(
    meta: Option<Res<MinimapMeta>>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
    mut q: Query<&mut Node, With<MinimapImage>>,
) {
    let Some(meta) = meta else { return };
    let Ok(wp) = player_q.single() else { return };
    let Ok(mut node) = q.single_mut() else { return };
    let p = wp.0;
    let tx = (p.x - meta.origin.x) / meta.tile; // тайлы, 0..w
    let jy = (p.y - meta.origin.y) / meta.tile; // тайлы, 0..h (мир +y вверх)
    let ty = meta.h as f32 - jy; // разворот: верх текстуры = макс. world.y
    node.left = Val::Px(MM_VIEW * 0.5 - tx * MM_SCALE);
    node.top = Val::Px(MM_VIEW * 0.5 - ty * MM_SCALE);
}

/// Тоггл миникарты по `M`.
pub fn toggle_minimap(
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<&mut Visibility, With<MinimapRoot>>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    for mut v in q.iter_mut() {
        *v = match *v {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
    }
}
