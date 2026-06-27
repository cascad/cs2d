//! Внутриигровое меню по Esc (пауза-оверлей). Пока содержит единственный пункт —
//! «Выход». Пункт активируется мышью (клик) или клавишей Enter. Esc открывает и
//! закрывает меню. Геймплей при этом не останавливается (игра сетевая), меню —
//! просто оверлей поверх HUD.

use bevy::prelude::*;
use bevy::ui::{
    AlignItems, BackgroundColor, FlexDirection, JustifyContent, Node, PositionType, UiRect, Val,
};

use crate::app_state::AppState;

/// Открыто ли меню паузы.
#[derive(Resource, Default)]
pub struct PauseMenuState {
    pub open: bool,
}

#[derive(Component)]
struct PauseRoot;

/// Кнопка-пункт «Выход».
#[derive(Component)]
struct PauseExitButton;

pub struct PauseMenuPlugin;
impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenuState>()
            .add_systems(
                Update,
                (
                    toggle_pause_menu,
                    pause_exit_interaction,
                    pause_menu_keyboard,
                )
                    .run_if(in_state(AppState::InGame)),
            )
            // при выходе из игры меню гарантированно закрываем
            .add_systems(OnExit(AppState::InGame), close_pause_menu);
    }
}

/// Esc — открыть/закрыть меню (спавним/деспавним оверлей).
fn toggle_pause_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PauseMenuState>,
    q_root: Query<Entity, With<PauseRoot>>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    state.open = !state.open;
    if state.open {
        spawn_pause_ui(&mut commands, &assets);
    } else {
        for e in &q_root {
            commands.entity(e).despawn();
        }
    }
}

/// Клик мышью по пункту «Выход» + подсветка при наведении.
fn pause_exit_interaction(
    mut q: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<PauseExitButton>)>,
    mut writer: MessageWriter<AppExit>,
) {
    for (interaction, mut bg) in &mut q {
        match *interaction {
            Interaction::Pressed => {
                writer.write(AppExit::Success);
            }
            Interaction::Hovered => *bg = BackgroundColor(EXIT_BG_HOVER),
            Interaction::None => *bg = BackgroundColor(EXIT_BG),
        }
    }
}

/// Enter — активировать выделенный пункт (пока он один — «Выход»).
fn pause_menu_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<PauseMenuState>,
    mut writer: MessageWriter<AppExit>,
) {
    if state.open && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)) {
        writer.write(AppExit::Success);
    }
}

fn close_pause_menu(
    mut state: ResMut<PauseMenuState>,
    q_root: Query<Entity, With<PauseRoot>>,
    mut commands: Commands,
) {
    state.open = false;
    for e in &q_root {
        commands.entity(e).despawn();
    }
}

const EXIT_BG: Color = Color::srgba(0.25, 0.12, 0.12, 1.0);
const EXIT_BG_HOVER: Color = Color::srgba(0.40, 0.16, 0.16, 1.0);

fn spawn_pause_ui(commands: &mut Commands, assets: &AssetServer) {
    let font = assets.load("fonts/FiraSans-Regular.ttf");

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(900),
            PauseRoot,
        ))
        .with_children(|root| {
            // Карточка меню
            root.spawn((
                Node {
                    width: Val::Px(320.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    align_items: AlignItems::Stretch,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.12, 0.14, 0.18, 0.98)),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new("Меню"),
                    TextFont {
                        font: font.clone(),
                        font_size: 24.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));

                // Пункт «Выход»
                card.spawn((
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(EXIT_BG),
                    Interaction::None,
                    PauseExitButton,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Выход"),
                        TextFont {
                            font: font.clone(),
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });

                card.spawn((
                    Text::new("Enter — выбрать, Esc — закрыть"),
                    TextFont {
                        font: font.clone(),
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgba(0.6, 0.6, 0.65, 1.0)),
                ));
            });
        });
}
