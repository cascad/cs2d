use bevy::prelude::*;
use bevy::ui::{AlignItems, BackgroundColor, FlexDirection, JustifyContent, Node, UiRect, Val};
use bevy_quinnet::client::QuinnetClient;
use bevy_quinnet::client::certificate::CertificateVerificationMode;
use bevy_quinnet::client::connection::ClientEndpointConfiguration;
use protocol::quinnet_adapter::build_channels_config;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::app_state::AppState;
use crate::resources::CurrentConnId;

// ===== Ресурсы / компоненты =====

#[derive(Resource, Default, Clone)]
pub struct ServerAddr(pub String);

#[derive(Resource, Default)]
pub struct ConnectError(pub Option<String>); // хранит текст последней ошибки коннекта

#[derive(Resource)]
pub struct ConnectTimeout(pub Timer);

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MenuCamera;
#[derive(Component)]
struct AddrValue; // текст набранного адреса (моношрифт)
#[derive(Component)]
struct ConnectButton; // прямоугольник-кнопка
#[derive(Component)]
struct ErrorText; // текст ошибки

// ===== Плагин =====

pub struct MenuPlugin;
impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerAddr>()
            .init_resource::<ConnectError>()
            .add_systems(OnEnter(AppState::Menu), menu_setup)
            .add_systems(
                Update,
                (
                    menu_typing,          // ввод адреса + курсор
                    try_connect_enter,    // Enter → попытка коннекта с показом ошибки
                    click_connect_button, // клик по кнопке → то же
                    render_connect_error, // обновление текста ошибки
                )
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnExit(AppState::Menu), menu_cleanup);
    }
}

// ===== UI =====

fn menu_setup(mut commands: Commands, mut addr: ResMut<ServerAddr>, assets: Res<AssetServer>) {
    if addr.0.is_empty() {
        addr.0 = "127.0.0.1:6000".to_string();
    }

    // Камера для меню
    commands.spawn((Camera2d::default(), MenuCamera));

    // Корневой контейнер
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 1.0)),
            MenuRoot,
        ))
        .with_children(|root| {
            // Карточка
            root.spawn((
                Node {
                    width: Val::Px(560.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.12, 0.14, 0.18, 0.95)),
            ))
            .with_children(|card| {
                // Заголовок
                card.spawn((
                    Text::new("Подключение к серверу"),
                    TextFont {
                        font: assets.load("fonts/FiraSans-Regular.ttf"),
                        font_size: 28.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));

                // Ряд: метка + значение (моношрифт)
                card.spawn((
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.09, 0.12, 1.0)),
                ))
                .with_children(|row| {
                    // Метка
                    row.spawn((
                        Text::new("Адрес:"),
                        TextFont {
                            font: assets.load("fonts/FiraSans-Regular.ttf"),
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.85, 0.85, 0.9, 1.0)),
                    ));
                    // Значение + курсор
                    row.spawn((
                        Text::new(""),
                        TextFont {
                            font: assets.load("fonts/FiraMono-Medium.ttf"),
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        AddrValue,
                    ));
                });

                // Подсказка
                card.spawn((
                    Text::new(
                        "Введи адрес (например: 127.0.0.1:6000) и нажми Enter.\nEsc — выйти.",
                    ),
                    TextFont {
                        font: assets.load("fonts/FiraSans-Regular.ttf"),
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgba(0.8, 0.8, 0.85, 1.0)),
                ));

                // Текст ошибки (пустой, станет красным при ошибке)
                card.spawn((
                    Text::new(""),
                    TextFont {
                        font: assets.load("fonts/FiraSans-Regular.ttf"),
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 0.35, 0.35, 1.0)),
                    ErrorText,
                ));

                // Кнопка «Подключиться» (кликабельная благодаря Interaction)
                card.spawn((
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.15, 0.2, 0.3, 1.0)),
                    Interaction::None, // важно: иначе клик по любому UI зачтётся как нажатие кнопки
                    ConnectButton,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Подключиться"),
                        TextFont {
                            font: assets.load("fonts/FiraSans-Regular.ttf"),
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });
            });
        });
}

fn menu_cleanup(
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
    q_root: Query<Entity, With<MenuRoot>>,
    q_cam: Query<Entity, With<MenuCamera>>,
) {
    err.0 = None; // очистим ошибку при выходе из меню
    for e in &q_root {
        commands.entity(e).despawn();
    }
    for e in &q_cam {
        commands.entity(e).despawn();
    }
}

// ===== Ввод строки =====

fn menu_typing(
    mut addr: ResMut<ServerAddr>,
    mut q_value: Query<&mut Text, With<AddrValue>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    // Добавлялка
    let mut push_if = |kc: KeyCode, ch: char| {
        if keys.just_pressed(kc) && addr.0.len() < 64 {
            addr.0.push(ch);
        }
    };

    // 0..9
    push_if(KeyCode::Digit0, '0');
    push_if(KeyCode::Digit1, '1');
    push_if(KeyCode::Digit2, '2');
    push_if(KeyCode::Digit3, '3');
    push_if(KeyCode::Digit4, '4');
    push_if(KeyCode::Digit5, '5');
    push_if(KeyCode::Digit6, '6');
    push_if(KeyCode::Digit7, '7');
    push_if(KeyCode::Digit8, '8');
    push_if(KeyCode::Digit9, '9');

    // A..F (IPv6)
    push_if(KeyCode::KeyA, 'a');
    push_if(KeyCode::KeyB, 'b');
    push_if(KeyCode::KeyC, 'c');
    push_if(KeyCode::KeyD, 'd');
    push_if(KeyCode::KeyE, 'e');
    push_if(KeyCode::KeyF, 'f');

    // '.' и ':' (через ; — часто Shift+';')
    push_if(KeyCode::Period, '.');
    if keys.just_pressed(KeyCode::Semicolon) && addr.0.len() < 64 {
        addr.0.push(':');
    }

    // Backspace / Escape
    if keys.just_pressed(KeyCode::Backspace) {
        addr.0.pop();
    }
    if keys.just_pressed(KeyCode::Escape) {
        std::process::exit(0);
    }

    // Блинкер курсора
    let cursor = if (time.elapsed_secs() * 2.0).floor() as i32 % 2 == 0 {
        "|"
    } else {
        " "
    };

    if let Ok(mut t) = q_value.single_mut() {
        *t = Text::new(format!("{}{}", addr.0, cursor));
    }
}

// ===== Коннект по Enter =====

fn try_connect_enter(
    keys: Res<ButtonInput<KeyCode>>,
    addr: Res<ServerAddr>,
    mut client: ResMut<QuinnetClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    if !keys.just_pressed(KeyCode::Enter) || addr.0.is_empty() {
        return;
    }

    match do_connect(&addr.0, &mut client, &mut commands) {
        Ok(_) => {
            info!("✅ connected, going Connecting");
            err.0 = None;
            commands.insert_resource(ConnectTimeout(Timer::from_seconds(3.0, TimerMode::Once)));
            next.set(AppState::Connecting);
        }
        Err(e) => {
            info!("⛔ stay in Menu: {}", e);
            err.0 = Some(format!("Сервер не найден: {}", e));
        }
    }
}

// ===== Коннект по клику =====

fn click_connect_button(
    mut q_btn: Query<&Interaction, (Changed<Interaction>, With<ConnectButton>)>,
    addr: Res<ServerAddr>,
    mut client: ResMut<QuinnetClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    for interaction in &mut q_btn {
        if *interaction == Interaction::Pressed {
            if addr.0.is_empty() {
                return;
            }
            match do_connect(&addr.0, &mut client, &mut commands) {
                Ok(_) => {
                    info!("✅ connected, going Connecting");
                    err.0 = None;
                    commands
                        .insert_resource(ConnectTimeout(Timer::from_seconds(3.0, TimerMode::Once)));
                    next.set(AppState::Connecting);
                }
                Err(e) => {
                    info!("⛔ stay in Menu: {}", e);
                    err.0 = Some(format!("Сервер не найден: {}", e));
                }
            }
        }
    }
}

// ===== Отрисовка текста ошибки =====

fn render_connect_error(err: Res<ConnectError>, mut q: Query<&mut Text, With<ErrorText>>) {
    if !err.is_changed() {
        return;
    }
    if let Ok(mut t) = q.single_mut() {
        if let Some(msg) = &err.0 {
            *t = Text::new(msg.clone());
        } else {
            *t = Text::new(String::new());
        }
    }
}

// ===== Общая функция подключения (как у тебя в setup ранее) =====

fn do_connect(
    addr_str: &str,
    client: &mut ResMut<QuinnetClient>,
    commands: &mut Commands,
) -> Result<(), String> {
    let server_addr: SocketAddr = addr_str
        .parse()
        .map_err(|_| format!("неверный адрес: {addr_str}"))?;

    let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
    let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    let cert_mode = CertificateVerificationMode::SkipVerification;
    let channels_config = build_channels_config();

    match client.open_connection(endpoint_config, cert_mode, channels_config) {
        Ok(conn_id) => {
            commands.insert_resource(CurrentConnId(Some(conn_id)));
            info!("🔌 Подключаемся к {}", addr_str);
            Ok(())
        }
        Err(e) => Err(format!("{:?}", e)),
    }
}

pub fn connection_timeout_system(
    time: Res<Time>,
    mut client: ResMut<QuinnetClient>,
    mut next: ResMut<NextState<AppState>>,
    mut err: ResMut<ConnectError>,
    mut timeout: Option<ResMut<ConnectTimeout>>,
    conn_id: Option<Res<CurrentConnId>>,
    mut commands: Commands,
) {
    let Some(mut t) = timeout else {
        return;
    };
    t.0.tick(time.delta());
    if t.0.finished() {
        // закрываем текущее соединение (если знаем id), иначе на всякий
        if let Some(id) = conn_id.and_then(|c| c.0) {
            let _ = client.close_connection(id);
        } else {
            let _ = client.close_all_connections();
        }
        // сообщение и возврат в меню
        err.0 = Some("Таймаут подключения (сервер не отвечает)".into());
        next.set(AppState::Menu);
        // убираем таймер
        commands.remove_resource::<ConnectTimeout>();
    }
}

pub fn clear_connect_timeout(mut commands: Commands) {
    commands.remove_resource::<ConnectTimeout>();
}
