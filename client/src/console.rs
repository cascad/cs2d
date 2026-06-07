//! Внутриигровая консоль логов: открывается по `~` полупрозрачным оверлеем.
//!
//! Логи захватываются «правильно» — через свой слой `tracing` (тот же канал,
//! что и макросы `info!`/`warn!`/`error!`). Слой пишет строки в общий буфер
//! (`Arc<Mutex<..>>`), а UI раз в кадр показывает последние N строк.
//!
//! Важно: захватываются события `tracing` (`info!`/`warn!`/`error!`/`debug!`),
//! а НЕ `println!` — последние идут мимо подписчика прямо в stdout.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::{Event, Level, Subscriber};
use bevy::log::tracing_subscriber::layer::Context;
use bevy::log::tracing_subscriber::Layer;
use bevy::log::BoxedLayer;
use bevy::prelude::*;

const MAX_LINES: usize = 1000;
const VISIBLE_LINES: usize = 24;

#[derive(Clone)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

/// Кольцевой буфер строк лога, общий между слоем `tracing` и UI.
#[derive(Resource, Clone)]
pub struct ConsoleLog(pub Arc<Mutex<VecDeque<LogLine>>>);

#[derive(Resource, Default)]
pub struct ConsoleState {
    pub open: bool,
}

#[derive(Component)]
pub struct ConsoleRoot;

#[derive(Component)]
pub struct ConsoleTextMarker;

/// Хук для `LogPlugin.custom_layer`: создаёт буфер, кладёт ресурсы в App и
/// возвращает слой, который пишет события в этот буфер. Вызывается один раз при
/// сборке приложения.
pub fn capture_console_layer(app: &mut App) -> Option<BoxedLayer> {
    let buf: Arc<Mutex<VecDeque<LogLine>>> = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LINES)));
    app.insert_resource(ConsoleLog(buf.clone()));
    app.insert_resource(ConsoleState::default());
    Some(Box::new(CaptureLayer { buf }))
}

struct CaptureLayer {
    buf: Arc<Mutex<VecDeque<LogLine>>>,
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write as _;
            self.message.clear();
            let _ = write!(self.message, "{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut v = MessageVisitor::default();
        event.record(&mut v);
        if v.message.is_empty() {
            return;
        }
        let line = LogLine {
            level: *meta.level(),
            text: v.message,
        };
        if let Ok(mut b) = self.buf.lock() {
            b.push_back(line);
            while b.len() > MAX_LINES {
                b.pop_front();
            }
        }
    }
}

fn level_tag(l: Level) -> &'static str {
    match l {
        Level::ERROR => "[E]",
        Level::WARN => "[W]",
        Level::INFO => "[I]",
        Level::DEBUG => "[D]",
        Level::TRACE => "[T]",
    }
}

fn level_color(l: Level) -> Color {
    match l {
        Level::ERROR => Color::srgb(1.0, 0.45, 0.45),
        Level::WARN => Color::srgb(1.0, 0.85, 0.4),
        Level::INFO => Color::srgb(0.65, 1.0, 0.65),
        Level::DEBUG => Color::srgb(0.6, 0.8, 1.0),
        Level::TRACE => Color::srgb(0.7, 0.7, 0.7),
    }
}

/// Оверлей создаём один раз на старте (скрыт по умолчанию), чтобы консоль
/// работала в любом состоянии (меню/игра).
pub fn setup_console_ui(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(45.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.05, 0.86)),
            GlobalZIndex(1000),
            Visibility::Hidden,
            ConsoleRoot,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("~ console — нажми ~, чтобы закрыть"),
                TextFont {
                    font,
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgb(0.65, 1.0, 0.65)),
                ConsoleTextMarker,
            ));
        });
}

/// Тоггл по клавише `~` (физическая Backquote).
pub fn toggle_console(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ConsoleState>,
    mut q: Query<&mut Visibility, With<ConsoleRoot>>,
) {
    if !keys.just_pressed(KeyCode::Backquote) {
        return;
    }
    state.open = !state.open;
    if let Ok(mut vis) = q.single_mut() {
        *vis = if state.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Пока консоль открыта — обновляем текст последними строками буфера.
/// Цвет берём по преобладающему уровню последней пачки (компромисс: один
/// `TextColor` на блок; разноцветный per-line потребовал бы дочерних узлов).
pub fn update_console_ui(
    state: Res<ConsoleState>,
    log: Res<ConsoleLog>,
    mut q: Query<(&mut Text, &mut TextColor), With<ConsoleTextMarker>>,
) {
    if !state.open {
        return;
    }
    let Ok((mut text, mut color)) = q.single_mut() else {
        return;
    };
    let Ok(buf) = log.0.lock() else {
        return;
    };

    let start = buf.len().saturating_sub(VISIBLE_LINES);
    let mut s = String::with_capacity(2048);
    let mut worst = Level::TRACE;
    for line in buf.iter().skip(start) {
        s.push_str(level_tag(line.level));
        s.push(' ');
        s.push_str(&line.text);
        s.push('\n');
        // worst = самый «важный» уровень (ERROR < WARN < INFO в смысле серьёзности,
        // но у tracing ERROR — наименьший по значению, поэтому берём min).
        if line.level < worst {
            worst = line.level;
        }
    }
    if s.is_empty() {
        s.push_str("(пусто) логи появятся здесь");
        worst = Level::INFO;
    }
    text.0 = s;
    color.0 = level_color(worst);
}
