//! Панель активных эффектов (дебаффов) ЛОКАЛЬНОГО игрока — слева на экране.
//! Сейчас единственный эффект — оглушение (stun): иконка со звёздочками и полоска
//! отката под ней, показывающая, сколько стана осталось. Пока стан активен —
//! значок виден; новый стан сбрасывает полоску в полную (длительность считается от
//! `STUN_DURATION`); как стан кончился — значок и полоска прячутся.

use bevy::prelude::*;

use crate::resources::{LocalStatus, UiFont};
use protocol::constants::STUN_DURATION;

/// Контейнер слота стана (вкл./выкл. видимость целиком).
#[derive(Component)]
pub struct StunEffectRoot;

/// Заливка полоски оставшегося времени стана (меняем ширину).
#[derive(Component)]
pub struct StunEffectBar;

pub fn setup_status_effects_ui(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(150.0),
                width: Val::Px(48.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            Visibility::Hidden,
            StunEffectRoot,
            crate::components::SessionScoped,
            Name::new("StatusEffect: Stun"),
        ))
        .with_children(|slot| {
            // иконка-квадрат со звёздочками
            slot.spawn((
                Node {
                    width: Val::Px(46.0),
                    height: Val::Px(46.0),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.12, 0.10, 0.18, 0.82)),
            ))
            .with_children(|icon| {
                icon.spawn((
                    Text::new("\u{2605}\u{2605}\u{2605}"),
                    TextFont {
                        font: font.0.clone(),
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.9, 0.25)),
                ));
            });
            // полоска оставшегося времени под значком
            slot.spawn((
                Node {
                    width: Val::Px(46.0),
                    height: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.15, 0.15, 0.18, 0.6)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.7, 0.4, 0.95, 0.95)),
                    StunEffectBar,
                ));
            });
        });
}

pub fn update_status_effects_ui(
    status: Res<LocalStatus>,
    mut root_q: Query<&mut Visibility, With<StunEffectRoot>>,
    mut bar_q: Query<&mut Node, With<StunEffectBar>>,
) {
    let active = status.stun_left > 0.0;
    for mut vis in &mut root_q {
        *vis = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    // полоска = доля оставшегося времени (новый стан сбрасывает в 100%)
    let frac = (status.stun_left / STUN_DURATION).clamp(0.0, 1.0);
    for mut node in &mut bar_q {
        node.width = Val::Percent(frac * 100.0);
    }
}
