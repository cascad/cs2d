//! Индикаторы кулдаунов рывка (Space) и ближнего удара (Mouse1). Берём значения
//! из локального предсказания способностей (`LocalAbilities`) — то же, что
//! считает сервер, так что полоска совпадает с реальной готовностью.

use bevy::prelude::*;

use crate::resources::{LocalAbilities, UiFont};
use protocol::constants::{DASH_COOLDOWN, MELEE_COOLDOWN, STUN_COOLDOWN};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CdKind {
    Dash,
    Melee,
    Stun,
}

#[derive(Component)]
pub struct CooldownBar(pub CdKind);

fn spawn_indicator(commands: &mut Commands, font: &Handle<Font>, bottom: f32, label: &str, kind: CdKind) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(bottom),
            right: Val::Px(20.0),
            width: Val::Px(164.0),
            height: Val::Px(14.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .insert(crate::components::SessionScoped)
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font: font.clone(),
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
                    height: Val::Px(12.0),
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
                    BackgroundColor(Color::srgba(0.4, 0.9, 0.5, 0.9)),
                    CooldownBar(kind),
                ));
            });
        });
}

pub fn setup_cooldowns_ui(mut commands: Commands, font: Res<UiFont>) {
    // над стаминой (42) и гранатой (20): рывок 64, удар 86, стан (Q) 108
    spawn_indicator(&mut commands, &font.0, 64.0, "DASH", CdKind::Dash);
    spawn_indicator(&mut commands, &font.0, 86.0, "ATK", CdKind::Melee);
    spawn_indicator(&mut commands, &font.0, 108.0, "STUN", CdKind::Stun);
}

pub fn update_cooldowns_ui(
    abil: Res<LocalAbilities>,
    mut q: Query<(&CooldownBar, &mut Node, &mut BackgroundColor)>,
) {
    for (bar, mut node, mut bg) in &mut q {
        let (cd_left, cd_max) = match bar.0 {
            CdKind::Dash => (abil.0.dash_cd_left, DASH_COOLDOWN),
            CdKind::Melee => (abil.0.melee_cd_left, MELEE_COOLDOWN),
            CdKind::Stun => (abil.0.stun_cd_left, STUN_COOLDOWN),
        };
        let ready = cd_left <= 0.0;
        let frac = if ready {
            1.0
        } else {
            (1.0 - (cd_left / cd_max)).clamp(0.0, 1.0)
        };
        node.width = Val::Percent(frac * 100.0);
        bg.0 = if ready {
            Color::srgba(0.35, 0.95, 0.5, 0.92) // готово — ярко-зелёный
        } else {
            Color::srgba(0.85, 0.6, 0.25, 0.85) // на кулдауне — янтарный
        };
    }
}
