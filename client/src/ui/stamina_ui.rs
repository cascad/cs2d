use bevy::prelude::*;

use crate::resources::LocalStatus;
use protocol::constants::STAMINA_MAX;

#[derive(Component)]
pub struct StaminaBar;

/// Полоска стамины над полоской гранаты (правый нижний угол).
pub fn setup_stamina_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Px(120.0),
                height: Val::Px(10.0),
                position_type: PositionType::Absolute,
                bottom: Val::Px(42.0),
                right: Val::Px(20.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(Color::srgba(0.2, 0.2, 0.2, 0.5)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    display: Display::Flex,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.95, 0.85, 0.2, 0.9)),
                StaminaBar,
            ));
        });
}

pub fn update_stamina_ui(
    status: Res<LocalStatus>,
    mut query: Query<(&mut Node, &mut BackgroundColor), With<StaminaBar>>,
) {
    let frac = (status.stamina / STAMINA_MAX).clamp(0.0, 1.0);
    // при активном блоке подсвечиваем полоску голубым
    let color = if status.blocking {
        Color::srgba(0.4, 0.7, 1.0, 0.95)
    } else {
        Color::srgba(0.95, 0.85, 0.2, 0.9)
    };
    for (mut node, mut bg) in &mut query {
        node.width = Val::Percent(frac * 100.0);
        bg.0 = color;
    }
}
