use bevy::prelude::*;

use crate::resources::UiFont;
use crate::ui::update_grenade_cooldown_ui::GrenadeCooldownBar;

/// Полоска КД гранаты (правый нижний угол, под стаминой), с подписью — в одном
/// стиле с индикаторами DASH/ATK/STUN из `cooldowns_ui`.
pub fn setup_grenade_ui(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(20.0),
            right: Val::Px(20.0),
            width: Val::Px(164.0),
            height: Val::Px(16.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("NADE"),
                TextFont {
                    font: font.0.clone(),
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgba(0.9, 0.9, 0.9, 0.95)),
                Node {
                    width: Val::Px(38.0),
                    ..default()
                },
            ));
            row.spawn((
                Node {
                    width: Val::Px(120.0),
                    height: Val::Px(14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.15, 0.15, 0.18, 0.55)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(80.0 / 255.0, 160.0 / 255.0, 1.0, 0.8)),
                    GrenadeCooldownBar,
                ));
            });
        });
}
