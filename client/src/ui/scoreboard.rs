//! Таблица очков (как в CS): полупрозрачный оверлей во весь экран, виден ТОЛЬКО
//! пока зажат Tab. Сквозь него видно игру (фон с альфой). Данные приходят с
//! сервера (`S2C::Scoreboard`) и привязаны к аккаунту, поэтому переживают
//! реконнект, пока жив сервер.

use bevy::prelude::*;

use crate::resources::{MyPlayer, ScoreboardData, UiFont};

/// Корневой полупрозрачный оверлей (тогглим Visibility по Tab).
#[derive(Component)]
pub struct ScoreboardRoot;

/// Контейнер строк — его детей пересобираем из данных, пока таблица видна.
#[derive(Component)]
pub struct ScoreboardRows;

const COL_KILLS: f32 = 70.0;
const COL_NPC: f32 = 70.0;
const COL_DEATHS: f32 = 70.0;

pub fn setup_scoreboard_ui(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            // лёгкая вуаль на весь экран — игра остаётся читаемой сквозь неё
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            Visibility::Hidden,
            ScoreboardRoot,
            GlobalZIndex(50),
        ))
        .with_children(|root| {
            // панель таблицы
            root.spawn((
                Node {
                    width: Val::Px(560.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(14.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.08, 0.10, 0.86)),
            ))
            .with_children(|panel| {
                // заголовок таблицы
                panel.spawn((
                    Text::new("ТАБЛИЦА ОЧКОВ"),
                    TextFont {
                        font: font.0.clone(),
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.9, 0.5)),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                ));

                // строка-шапка колонок
                spawn_header(panel, &font);

                // контейнер строк (пересобирается каждый кадр, пока видно)
                panel.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    ScoreboardRows,
                ));
            });
        });
}

fn spawn_header(panel: &mut ChildSpawnerCommands<'_>, font: &UiFont) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)),
        ))
        .with_children(|row| {
            cell(row, font, "Игрок", None, 0.7, Color::srgb(0.7, 0.8, 0.9));
            cell(row, font, "Убийства", Some(COL_KILLS), 1.0, Color::srgb(0.7, 0.8, 0.9));
            cell(row, font, "Непись", Some(COL_NPC), 1.0, Color::srgb(0.7, 0.8, 0.9));
            cell(row, font, "Смерти", Some(COL_DEATHS), 1.0, Color::srgb(0.7, 0.8, 0.9));
        });
}

/// Ячейка строки. `width=None` → растягивается (имя); иначе фиксированная.
fn cell(
    parent: &mut ChildSpawnerCommands<'_>,
    font: &UiFont,
    text: &str,
    width: Option<f32>,
    _flex: f32,
    color: Color,
) {
    let mut node = Node {
        align_items: AlignItems::Center,
        ..default()
    };
    match width {
        Some(w) => node.width = Val::Px(w),
        None => node.flex_grow = 1.0,
    }
    parent.spawn((
        node,
        Text::new(text),
        TextFont {
            font: font.0.clone(),
            font_size: 16.0,
            ..default()
        },
        TextColor(color),
    ));
}

/// Тоггл видимости по Tab + пересборка строк из последних данных сервера.
pub fn update_scoreboard_ui(
    keys: Res<ButtonInput<KeyCode>>,
    data: Res<ScoreboardData>,
    me: Res<MyPlayer>,
    font: Res<UiFont>,
    mut commands: Commands,
    mut root_q: Query<&mut Visibility, With<ScoreboardRoot>>,
    rows_q: Query<Entity, With<ScoreboardRows>>,
    children_q: Query<&Children>,
) {
    let show = keys.pressed(KeyCode::Tab);

    for mut vis in root_q.iter_mut() {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !show {
        return;
    }

    let Ok(rows) = rows_q.single() else {
        return;
    };

    // очищаем прошлые строки
    if let Ok(children) = children_q.get(rows) {
        for c in children.iter() {
            commands.entity(c).despawn();
        }
    }

    // строим заново из данных
    commands.entity(rows).with_children(|rows| {
        for (i, e) in data.0.iter().enumerate() {
            let is_me = e.online && e.id == me.id;
            let bg = if is_me {
                Color::srgba(0.95, 0.85, 0.3, 0.18)
            } else if i % 2 == 0 {
                Color::srgba(1.0, 1.0, 1.0, 0.03)
            } else {
                Color::NONE
            };
            let name_color = if is_me {
                Color::srgb(1.0, 0.95, 0.55)
            } else if e.online {
                Color::WHITE
            } else {
                Color::srgb(0.55, 0.55, 0.6) // оффлайн — приглушённо
            };

            rows.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(bg),
            ))
            .with_children(|row| {
                let label = if e.online {
                    e.name.clone()
                } else {
                    format!("{} (оффлайн)", e.name)
                };
                cell(row, &font, &label, None, 0.7, name_color);
                cell(row, &font, &e.kills.to_string(), Some(COL_KILLS), 1.0, name_color);
                cell(row, &font, &e.npc_kills.to_string(), Some(COL_NPC), 1.0, name_color);
                cell(row, &font, &e.deaths.to_string(), Some(COL_DEATHS), 1.0, name_color);
            });
        }
    });
}
