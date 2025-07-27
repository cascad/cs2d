use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use bevy::sprite::Sprite;

/// Сообщение, которое отправляем на сервер и получаем обратно для broadcast
#[derive(Serialize, Deserialize, Clone, Debug, Event)]
pub struct CommandEvent {
    pub position: Vec2,
}

pub struct SimpleGamePlugin;

impl Plugin for SimpleGamePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<CommandEvent>()
            .add_systems(Startup, setup_camera)
            .add_systems(Update, (detect_click_and_send, draw_on_click));
    }
}

/// Камера для 2D-сцены
fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d::default());
}

/// Локально детектим клик и создаём событие (потом уйдёт на сервер)
fn detect_click_and_send(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut ev_writer: EventWriter<CommandEvent>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        if let Ok(win) = windows.single() {
            if let Some(cursor) = win.cursor_position() {
                // Переводим экранные координаты в мировые (с центром в 0,0)
                let pos = Vec2::new(cursor.x - win.width() / 2., win.height() / 2. - cursor.y);
                println!("🖱 Click at: {:?}", pos);
                ev_writer.write(CommandEvent { position: pos });
            }
        }
    }
}

/// Рисуем квадратик в позиции клика (когда пришло событие — локальное или из сети)
fn draw_on_click(mut commands: Commands, mut ev_reader: EventReader<CommandEvent>) {
    for ev in ev_reader.read() {
        commands.spawn((
            Sprite {
                color: Color::srgb(0.3, 0.8, 0.3),
                custom_size: Some(Vec2::splat(40.0)),
                ..default()
            },
            Transform::from_translation(ev.position.extend(0.0)),
        ));
    }
}