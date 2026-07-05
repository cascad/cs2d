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

// Колонки должны быть шире самых длинных заголовков («Убийства»), иначе текст
// вылезает за ячейку и «слипается» с соседним. Числа центрируем под шапкой.
const COL_KILLS: f32 = 104.0;
const COL_NPC: f32 = 104.0;
const COL_DEATHS: f32 = 96.0;
const COL_NAME_MIN: f32 = 150.0;

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
            crate::components::SessionScoped,
            GlobalZIndex(50),
        ))
        .with_children(|root| {
            // ряд из двух блоков: слева табло, справа памятка управления;
            // верхние кромки на одной линии, весь ряд центрирован на экране
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(20.0),
                ..default()
            })
            .with_children(|row_of_panels| {
                // панель таблицы
                row_of_panels
                    .spawn((
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

                // памятка управления (статичная, собирается один раз)
                spawn_hotkeys_panel(row_of_panels, &font);
            });
        });
}

/// Памятка хоткеев — второй блок на Tab-оверлее. Клавиши слева жёлтым,
/// действия справа. Держать В СИНХРОНЕ с реальными биндингами:
/// `lynet::latch_discrete_input` / `lynet::buffer_input`.
fn spawn_hotkeys_panel(parent: &mut ChildSpawnerCommands<'_>, font: &UiFont) {
    const KEYS: [(&str, &str); 7] = [
        ("WASD", "движение"),
        ("ЛКМ", "атака"),
        ("ПКМ", "блок (держать)"),
        ("Пробел", "перекат"),
        ("Q", "стан"),
        ("G", "граната (отпустить — бросок)"),
        ("Tab", "это табло"),
    ];
    parent
        .spawn((
            Node {
                width: Val::Px(320.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.08, 0.10, 0.86)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("УПРАВЛЕНИЕ"),
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
            for (key, action) in KEYS {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(84.0),
                                ..default()
                            },
                            Text::new(key),
                            TextFont {
                                font: font.0.clone(),
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(Color::srgb(1.0, 0.9, 0.45)),
                        ));
                        row.spawn((
                            Text::new(action),
                            TextFont {
                                font: font.0.clone(),
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.85, 0.88, 0.92)),
                        ));
                    });
            }
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
        // фиксированные числовые колонки — текст по центру под заголовком
        Some(w) => {
            node.width = Val::Px(w);
            node.justify_content = JustifyContent::Center;
        }
        // колонка имени тянется, но не уже минимума (иначе длинные имена давят числа)
        None => {
            node.flex_grow = 1.0;
            node.min_width = Val::Px(COL_NAME_MIN);
        }
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

    // строим заново из данных — ТОЛЬКО те, кто сейчас на сервере (аккаунты
    // оффлайн-игроков сервер тоже шлёт, но в таблице они лишь шумели)
    commands.entity(rows).with_children(|rows| {
        for (i, e) in data.0.iter().filter(|e| e.online).enumerate() {
            let is_me = e.id == me.id;
            let bg = if is_me {
                Color::srgba(0.95, 0.85, 0.3, 0.18)
            } else if i % 2 == 0 {
                Color::srgba(1.0, 1.0, 1.0, 0.03)
            } else {
                Color::NONE
            };
            let name_color = if is_me {
                Color::srgb(1.0, 0.95, 0.55)
            } else {
                Color::WHITE
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
                cell(row, &font, &e.name, None, 0.7, name_color);
                cell(row, &font, &e.kills.to_string(), Some(COL_KILLS), 1.0, name_color);
                cell(row, &font, &e.npc_kills.to_string(), Some(COL_NPC), 1.0, name_color);
                cell(row, &font, &e.deaths.to_string(), Some(COL_DEATHS), 1.0, name_color);
            });
        }
    });
}
