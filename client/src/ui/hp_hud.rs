use bevy::prelude::*;

use crate::resources::{LocalStatus, UiFont};
use crate::systems::utils::hp_color;
use protocol::constants::PLAYER_MAX_HP;

#[derive(Component)]
pub struct HpHudBar;

#[derive(Component)]
pub struct HpHudText;

/// HUD здоровья локального игрока: подложка + заливка + число. Внизу справа, над
/// индикаторами кулдаунов/стамины (рядом с остальным HUD).
pub fn setup_hp_hud(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                width: Val::Px(164.0),
                height: Val::Px(20.0),
                position_type: PositionType::Absolute,
                bottom: Val::Px(110.0),
                right: Val::Px(20.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.6)),
        ))
        .with_children(|parent| {
            // заливка HP (растёт слева направо), на всю высоту, абсолютно слева
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(hp_color(1.0)),
                HpHudBar,
            ));
            // число поверх заливки, по центру
            parent.spawn((
                Text::new(format!("HP {} / {}", PLAYER_MAX_HP, PLAYER_MAX_HP)),
                TextFont {
                    font: font.0.clone(),
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                HpHudText,
            ));
        });
}

pub fn update_hp_hud(
    status: Res<LocalStatus>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), With<HpHudBar>>,
    mut text: Query<&mut Text, With<HpHudText>>,
) {
    let hp = status.hp.max(0);
    let frac = (hp as f32 / PLAYER_MAX_HP as f32).clamp(0.0, 1.0);
    for (mut node, mut bg) in bar.iter_mut() {
        node.width = Val::Percent(frac * 100.0);
        bg.0 = hp_color(frac);
    }
    for mut t in text.iter_mut() {
        t.0 = format!("HP {} / {}", hp, PLAYER_MAX_HP);
    }
}
