use bevy::prelude::*;

use crate::ui::update_grenade_cooldown_ui::GrenadeCooldownBar;

pub fn setup_grenade_ui(mut commands: Commands) {
    // создаём родительский UI-элемент (полоска фона)
    commands
        .spawn((
            Node {
                width: Val::Px(120.0),
                height: Val::Px(16.0),
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                right: Val::Px(20.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(Color::linear_rgba(
                80.0 / 255.0,
                160.0 / 255.0,
                255.0 / 255.0,
                0.5,
            )), // 128 - 50% прозрачность
        ))
        .with_children(|parent| {
            // внутренняя зелёная часть (заполнение)
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    display: Display::Flex,
                    ..default()
                },
                BackgroundColor(Color::linear_rgba(
                    80.0 / 255.0,
                    160.0 / 255.0,
                    255.0 / 255.0,
                    0.8,
                )),
                GrenadeCooldownBar,
            ));
        });
}
