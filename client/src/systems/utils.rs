use bevy::prelude::*;
use protocol::constants::{MOVE_SPEED, TICK_DT};
use protocol::messages::{InputState, Stance};

use crate::resources::UiFont;
use crate::ui::components::PlayerHpUi;

pub fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    now.as_secs_f64()
}

pub fn simulate_input(t: &mut Transform, inp: &InputState) {
    let mut dir = Vec2::ZERO;
    if inp.up {
        dir.y += 1.0;
    }
    if inp.down {
        dir.y -= 1.0;
    }
    if inp.left {
        dir.x -= 1.0;
    }
    if inp.right {
        dir.x += 1.0;
    }
    dir = dir.normalize_or_zero();
    t.translation += (dir * MOVE_SPEED * TICK_DT).extend(0.0);
    t.rotation = Quat::from_rotation_z(inp.rotation);
}

pub fn stance_color(s: &Stance) -> Color {
    match s {
        Stance::Standing => Color::srgb(0.20, 1.00, 0.20),
        Stance::Crouching => Color::srgb(0.15, 0.85, 1.00),
        Stance::Prone => Color::srgb(0.00, 0.60, 0.60),
    }
}

pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}

pub fn spawn_hp_ui(commands: &mut Commands, player_id: u64, hp: u32, font: Handle<Font>) {
    commands.spawn((
        Text2d(format!("{} HP", hp)),
        TextFont {
            font: font.into(),
            font_size: 14.0,
            ..Default::default()
        },
        TextColor(Color::WHITE.into()),
        PlayerHpUi { player_id },
    ));
}