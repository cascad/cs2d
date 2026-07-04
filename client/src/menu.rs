use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::ui::{AlignItems, BackgroundColor, FlexDirection, JustifyContent, Node, UiRect, Val};
use std::net::SocketAddr;

use crate::app_state::AppState;
#[cfg(not(target_arch = "wasm32"))]
use crate::lobby::{
    drain_query_results, spawn_server_queries, LobbyServer, LobbyServers, QueryInbox, ServerStatus,
};
use crate::config::ClientConfig;
use crate::lynet::{connect_to, LyClient};

// ===== Ресурсы / компоненты =====

#[derive(Resource, Default, Clone)]
pub struct ServerAddr(pub String);

#[derive(Resource, Default)]
pub struct ConnectError(pub Option<String>); // хранит текст последней ошибки коннекта

#[derive(Resource)]
pub struct ConnectTimeout(pub Timer);

/// Таймер периодического переопроса серверов в лобби (авто-рефреш статусов).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct LobbyRefreshTimer(Timer);
#[cfg(not(target_arch = "wasm32"))]
impl Default for LobbyRefreshTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(3.0, TimerMode::Repeating))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct MenuRoot;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct MenuCamera;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct AddrValue; // текст набранного адреса (моношрифт)
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ConnectButton; // прямоугольник-кнопка ручного ввода
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ErrorText; // текст ошибки

/// Кликабельная строка сервера из лобби. Хранит индекс в `LobbyServers` и адрес.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ServerRowButton {
    index: usize,
    address: String,
}
/// Текст имени сервера в строке (обновляется при получении меты).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ServerRowName(usize);
/// Текст статуса сервера в строке (проверка/онлайн N/M/офлайн).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ServerRowStatus(usize);

// ===== Плагин =====

pub struct MenuPlugin;
impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerAddr>()
            .init_resource::<ConnectError>();
        #[cfg(not(target_arch = "wasm32"))]
        app.init_resource::<LobbyServers>()
            .init_resource::<QueryInbox>()
            .init_resource::<LobbyRefreshTimer>();
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(OnEnter(AppState::Menu), menu_setup)
            .add_systems(
                Update,
                (
                    menu_typing,
                    try_connect_enter,
                    click_connect_button,
                    click_server_row,
                    auto_refresh_lobby,
                    drain_query_results,
                    refresh_lobby_ui,
                    render_connect_error,
                )
                    .run_if(in_state(AppState::Menu)),
            );
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(OnExit(AppState::Menu), menu_cleanup);
    }
}

// ===== UI (native) =====

#[cfg(not(target_arch = "wasm32"))]
fn menu_setup(
    mut commands: Commands,
    mut addr: ResMut<ServerAddr>,
    mut servers: ResMut<LobbyServers>,
    mut refresh: ResMut<LobbyRefreshTimer>,
    inbox: Res<QueryInbox>,
    cfg: Res<crate::config::ClientConfig>,
    assets: Res<AssetServer>,
) {
    // следующий авто-рефреш — через полный интервал после первого опроса
    refresh.0.reset();
    if addr.0.is_empty() {
        // адрес по умолчанию берём из конфига рядом с бинарём (client_config.toml)
        addr.0 = cfg.address();
    }

    // Строим список серверов лобби из конфига (статус — «проверка»), чистим
    // инбокс и запускаем фоновый опрос каждого сервера.
    servers.0 = cfg
        .servers
        .iter()
        .map(|s| LobbyServer {
            name: s.name.clone().unwrap_or_else(|| s.address.clone()),
            address: s.address.clone(),
            status: ServerStatus::Checking,
        })
        .collect();
    if let Ok(mut buf) = inbox.0.lock() {
        buf.clear();
    }
    spawn_server_queries(&servers, &inbox);

    let font = assets.load("fonts/FiraSans-Regular.ttf");
    let mono = assets.load("fonts/FiraMono-Medium.ttf");

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
            // Карточка лобби
            root.spawn((
                Node {
                    width: Val::Px(620.0),
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
                    Text::new("Выбор сервера"),
                    TextFont {
                        font: font.clone(),
                        font_size: 28.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));

                // ── Список серверов ───────────────────────────────────────
                if servers.0.is_empty() {
                    card.spawn((
                        Text::new(
                            "В client_config.toml нет серверов.\nДобавь [[servers]] или введи адрес вручную ниже.",
                        ),
                        TextFont {
                            font: font.clone(),
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.8, 0.8, 0.85, 1.0)),
                    ));
                } else {
                    card.spawn((Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    },))
                        .with_children(|list| {
                            for (i, s) in servers.0.iter().enumerate() {
                                spawn_server_row(list, i, s, &font, &mono);
                            }
                        });
                }

                // ── Разделитель / ручной ввод ─────────────────────────────
                card.spawn((
                    Text::new("Или подключись по адресу вручную:"),
                    TextFont {
                        font: font.clone(),
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::srgba(0.7, 0.7, 0.75, 1.0)),
                ));

                // Ряд: метка + значение (моношрифт) + кнопка
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
                    row.spawn((
                        Text::new("Адрес:"),
                        TextFont {
                            font: font.clone(),
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.85, 0.85, 0.9, 1.0)),
                    ));
                    row.spawn((
                        Text::new(""),
                        TextFont {
                            font: mono.clone(),
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        AddrValue,
                    ));
                });

                // Текст ошибки (пустой, станет красным при ошибке)
                card.spawn((
                    Text::new(""),
                    TextFont {
                        font: font.clone(),
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 0.35, 0.35, 1.0)),
                    ErrorText,
                ));

                // Кнопка «Подключиться» (ручной ввод)
                card.spawn((
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.15, 0.2, 0.3, 1.0)),
                    Interaction::None,
                    ConnectButton,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Подключиться по адресу"),
                        TextFont {
                            font: font.clone(),
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });

                // Подсказка
                card.spawn((
                    Text::new("Клик по серверу — подключиться. Enter — по адресу. Esc — выйти."),
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

/// Спавнит одну кликабельную строку сервера: слева имя, справа статус.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_server_row(
    list: &mut ChildSpawnerCommands<'_>,
    index: usize,
    s: &LobbyServer,
    font: &Handle<Font>,
    mono: &Handle<Font>,
) {
    list.spawn((
        Node {
            padding: UiRect::all(Val::Px(12.0)),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(12.0),
            ..default()
        },
        BackgroundColor(row_bg(&s.status)),
        Interaction::None,
        ServerRowButton {
            index,
            address: s.address.clone(),
        },
    ))
    .with_children(|row| {
        // Левая часть: имя + адрес (моноширинно)
        row.spawn((Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        },))
            .with_children(|col| {
                col.spawn((
                    Text::new(s.name.clone()),
                    TextFont {
                        font: font.clone(),
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    ServerRowName(index),
                ));
                col.spawn((
                    Text::new(s.address.clone()),
                    TextFont {
                        font: mono.clone(),
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgba(0.6, 0.62, 0.68, 1.0)),
                ));
            });
        // Правая часть: статус
        row.spawn((
            Text::new(status_text(&s.status)),
            TextFont {
                font: font.clone(),
                font_size: 16.0,
                ..default()
            },
            TextColor(status_color(&s.status)),
            ServerRowStatus(index),
        ));
    });
}

/// Текстовое представление статуса для правой части строки.
#[cfg(not(target_arch = "wasm32"))]
fn status_text(status: &ServerStatus) -> String {
    match status {
        ServerStatus::Checking => "проверка…".to_string(),
        ServerStatus::Online { players, max } => {
            if *max > 0 {
                format!("● онлайн   {players}/{max}")
            } else {
                format!("● онлайн   {players}")
            }
        }
        ServerStatus::Offline => "○ офлайн".to_string(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn status_color(status: &ServerStatus) -> Color {
    match status {
        ServerStatus::Checking => Color::srgba(0.75, 0.75, 0.5, 1.0),
        ServerStatus::Online { .. } => Color::srgba(0.4, 0.9, 0.45, 1.0),
        ServerStatus::Offline => Color::srgba(0.85, 0.4, 0.4, 1.0),
    }
}

/// Фон строки: онлайн подсвечиваем (зеленоватый), офлайн приглушаем.
#[cfg(not(target_arch = "wasm32"))]
fn row_bg(status: &ServerStatus) -> Color {
    match status {
        ServerStatus::Checking => Color::srgba(0.10, 0.11, 0.14, 1.0),
        ServerStatus::Online { .. } => Color::srgba(0.12, 0.20, 0.14, 1.0),
        ServerStatus::Offline => Color::srgba(0.10, 0.08, 0.09, 1.0),
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

/// Состояние авто-повтора Backspace (зажатие): задержка до старта и шаг повтора.
#[derive(Default)]
struct BackspaceRepeat {
    held: f32, // сколько уже удерживается
    acc: f32,  // накопитель для интервала повтора
}

#[cfg(not(target_arch = "wasm32"))]
fn menu_typing(
    mut addr: ResMut<ServerAddr>,
    mut q_value: Query<&mut Text, With<AddrValue>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut bs: Local<BackspaceRepeat>,
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

    // Backspace: первое нажатие стирает сразу; при удержании — авто-повтор после
    // короткой задержки (как в обычных полях ввода).
    const BS_INITIAL_DELAY: f32 = 0.35; // пауза перед автоповтором
    const BS_REPEAT_RATE: f32 = 0.04; // интервал автоповтора (≈25 симв/сек)
    if keys.just_pressed(KeyCode::Backspace) {
        addr.0.pop();
        bs.held = 0.0;
        bs.acc = 0.0;
    } else if keys.pressed(KeyCode::Backspace) {
        let dt = time.delta_secs();
        bs.held += dt;
        if bs.held >= BS_INITIAL_DELAY {
            bs.acc += dt;
            while bs.acc >= BS_REPEAT_RATE {
                bs.acc -= BS_REPEAT_RATE;
                if addr.0.pop().is_none() {
                    break;
                }
            }
        }
    } else {
        bs.held = 0.0;
        bs.acc = 0.0;
    }
    if keys.just_pressed(KeyCode::Escape) {
        #[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn try_connect_enter(
    keys: Res<ButtonInput<KeyCode>>,
    addr: Res<ServerAddr>,
    cfg: Res<ClientConfig>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    if !keys.just_pressed(KeyCode::Enter) || addr.0.is_empty() {
        return;
    }

    match do_connect(&addr.0, &mut commands, &mut ly, &cfg, None) {
        Ok(_) => {
            info!("✅ connected, going Connecting");
            err.0 = None;
            commands.insert_resource(ConnectTimeout(Timer::from_seconds(6.0, TimerMode::Once)));
            next.set(AppState::Connecting);
        }
        Err(e) => {
            info!("⛔ stay in Menu: {}", e);
            err.0 = Some(format!("Сервер не найден: {}", e));
        }
    }
}

// ===== Коннект по клику =====

#[cfg(not(target_arch = "wasm32"))]
fn click_connect_button(
    mut q_btn: Query<&Interaction, (Changed<Interaction>, With<ConnectButton>)>,
    addr: Res<ServerAddr>,
    cfg: Res<ClientConfig>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    for interaction in &mut q_btn {
        if *interaction == Interaction::Pressed {
            if addr.0.is_empty() {
                return;
            }
            match do_connect(&addr.0, &mut commands, &mut ly, &cfg, None) {
                Ok(_) => {
                    info!("✅ connected, going Connecting");
                    err.0 = None;
                    commands
                        .insert_resource(ConnectTimeout(Timer::from_seconds(6.0, TimerMode::Once)));
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

// ===== Коннект по клику на сервер из списка =====

#[cfg(not(target_arch = "wasm32"))]
fn click_server_row(
    mut q_btn: Query<(&Interaction, &ServerRowButton), Changed<Interaction>>,
    servers: Res<LobbyServers>,
    cfg: Res<ClientConfig>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    for (interaction, row) in &mut q_btn {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(s) = servers.0.get(row.index) {
            if s.status == ServerStatus::Offline {
                info!("сервер {} помечен офлайн, пробуем подключиться всё равно", row.address);
            }
        }
        match do_connect(&row.address, &mut commands, &mut ly, &cfg, None) {
            Ok(_) => {
                info!("✅ connected, going Connecting");
                err.0 = None;
                commands.insert_resource(ConnectTimeout(Timer::from_seconds(6.0, TimerMode::Once)));
                next.set(AppState::Connecting);
            }
            Err(e) => {
                info!("⛔ stay in Menu: {}", e);
                err.0 = Some(format!("Сервер не найден: {}", e));
            }
        }
    }
}

// ===== Авто-рефреш списка серверов =====

/// Периодически (по таймеру) перезапускает опрос всех серверов лобби, не сбрасывая
/// текущие статусы — чтобы не мигало, а просто обновлялось число игроков/доступность.
#[cfg(not(target_arch = "wasm32"))]
fn auto_refresh_lobby(
    time: Res<Time>,
    mut refresh: ResMut<LobbyRefreshTimer>,
    servers: Res<LobbyServers>,
    inbox: Res<QueryInbox>,
) {
    if servers.0.is_empty() {
        return;
    }
    if refresh.0.tick(time.delta()).just_finished() {
        spawn_server_queries(&servers, &inbox);
    }
}

// ===== Перерисовка статусов серверов в списке =====

#[cfg(not(target_arch = "wasm32"))]
fn refresh_lobby_ui(
    servers: Res<LobbyServers>,
    mut q_name: Query<(&ServerRowName, &mut Text), Without<ServerRowStatus>>,
    mut q_status: Query<(&ServerRowStatus, &mut Text, &mut TextColor), Without<ServerRowName>>,
    mut q_rows: Query<(&ServerRowButton, &mut BackgroundColor)>,
) {
    if !servers.is_changed() {
        return;
    }
    for (name, mut text) in &mut q_name {
        if let Some(s) = servers.0.get(name.0) {
            *text = Text::new(s.name.clone());
        }
    }
    for (status, mut text, mut color) in &mut q_status {
        if let Some(s) = servers.0.get(status.0) {
            *text = Text::new(status_text(&s.status));
            *color = TextColor(status_color(&s.status));
        }
    }
    for (row, mut bg) in &mut q_rows {
        if let Some(s) = servers.0.get(row.index) {
            *bg = BackgroundColor(row_bg(&s.status));
        }
    }
}

// ===== Отрисовка текста ошибки =====

#[cfg(not(target_arch = "wasm32"))]
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

pub fn do_connect(
    addr_str: &str,
    commands: &mut Commands,
    ly: &mut LyClient,
    cfg: &ClientConfig,
    runtime_digest: Option<&str>,
) -> Result<(), String> {
    let server_addr: SocketAddr = addr_str
        .parse()
        .map_err(|_| format!("неверный адрес: {addr_str}"))?;
    // На всякий случай гасим предыдущую сессию (повторный коннект без выхода).
    if let Some(prev) = ly.entity.take() {
        if let Ok(mut e) = commands.get_entity(prev) {
            e.despawn();
        }
    }
    let digest = cfg.connect_cert_digest(runtime_digest);
    let entity = connect_to(commands, server_addr, &digest);
    ly.entity = Some(entity);
    info!("🔌 Подключаемся к {} (lightyear)", addr_str);
    Ok(())
}

pub fn connection_timeout_system(
    time: Res<Time>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut err: ResMut<ConnectError>,
    timeout: Option<ResMut<ConnectTimeout>>,
    mut commands: Commands,
) {
    let Some(mut t) = timeout else {
        return;
    };
    t.0.tick(time.delta());
    if t.0.is_finished() {
        if let Some(prev) = ly.entity.take() {
            if let Ok(mut e) = commands.get_entity(prev) {
                e.despawn();
            }
        }
        err.0 = Some("Таймаут подключения (сервер не отвечает)".into());
        next.set(AppState::Menu);
        commands.remove_resource::<ConnectTimeout>();
    }
}

pub fn clear_connect_timeout(mut commands: Commands) {
    commands.remove_resource::<ConnectTimeout>();
}
