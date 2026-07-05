//! FPS-счётчик в углу экрана: живёт во всех состояниях (как консоль),
//! обновляется ~4 раза в секунду сглаженным значением из диагностик Bevy.
//! На wasm дополнительно подкрашивается: зелёный ≥ 50, жёлтый ≥ 30, красный ниже
//! — сразу видно «ватный» первый запуск (baseline-компиляция wasm) и прогрев.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

#[derive(Component)]
pub struct FpsText;

pub fn setup_fps_hud(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.spawn((
        Text::new("fps —"),
        TextFont {
            font,
            font_size: 13.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.9, 0.7, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(8.0),
            top: Val::Px(6.0),
            ..default()
        },
        GlobalZIndex(900),
        FpsText,
    ));
}

pub fn update_fps_hud(
    time: Res<Time>,
    mut acc: Local<f32>,
    diagnostics: Res<DiagnosticsStore>,
    mut q: Query<(&mut Text, &mut TextColor), With<FpsText>>,
) {
    *acc += time.delta_secs();
    if *acc < 0.25 {
        return;
    }
    *acc = 0.0;
    let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    else {
        return;
    };
    for (mut text, mut color) in &mut q {
        *text = Text::new(format!("fps {fps:.0}"));
        color.0 = if fps >= 50.0 {
            Color::srgba(0.65, 0.9, 0.65, 0.85)
        } else if fps >= 30.0 {
            Color::srgba(0.95, 0.85, 0.4, 0.9)
        } else {
            Color::srgba(1.0, 0.45, 0.4, 0.95)
        };
    }
}
