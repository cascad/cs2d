use bevy::prelude::*;

pub fn time_in_seconds() -> f64 {
    crate::platform::now_secs_f64()
}

pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}

use crate::components::HpFill;
use protocol::constants::PLAYER_MAX_HP;

/// Ширина/высота плавающей полоски HP над рыцарем (экранные px).
pub const HP_BAR_W: f32 = 48.0;
pub const HP_BAR_H: f32 = 6.0;

/// Плавающая полоска HP: тёмная подложка + цветная заливка (привязка к левому
/// краю, ужимается вправо при уроне). Возвращает РОДИТЕЛЯ — его двигает
/// `sync_hp_ui_position` (над головой), заливку обновляет `update_hp_text_from_event`.
pub fn spawn_hp_ui(commands: &mut Commands, player_id: u64, hp: i32) -> Entity {
    let frac = (hp as f32 / PLAYER_MAX_HP as f32).clamp(0.0, 1.0);
    commands
        .spawn((
            Transform::default(),
            GlobalTransform::default(),
            Visibility::Visible,
        ))
        .with_children(|p| {
            // тёмная рамка-подложка
            p.spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.65),
                    custom_size: Some(Vec2::new(HP_BAR_W + 2.0, HP_BAR_H + 2.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
            // заливка: якорь по левому краю, рост вправо
            p.spawn((
                Sprite {
                    color: hp_color(frac),
                    custom_size: Some(Vec2::new(HP_BAR_W * frac, HP_BAR_H)),
                    ..default()
                },
                bevy::sprite::Anchor(Vec2::new(-0.5, 0.0)),
                Transform::from_xyz(-HP_BAR_W * 0.5, 0.0, 0.1),
                HpFill { id: player_id, full_w: HP_BAR_W },
            ));
        })
        .id()
}

/// Цвет полоски HP: зелёный → жёлтый → красный по мере падения.
pub fn hp_color(frac: f32) -> Color {
    if frac > 0.5 {
        Color::srgb(0.3, 0.9, 0.35)
    } else if frac > 0.25 {
        Color::srgb(0.95, 0.8, 0.2)
    } else {
        Color::srgb(0.9, 0.25, 0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::lerp_angle;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn lerp_angle_midpoint() {
        assert!(approx(lerp_angle(0.0, FRAC_PI_2, 0.5), FRAC_PI_4));
    }

    #[test]
    fn lerp_angle_takes_short_way_around() {
        // от 0 к 3π/2 короче идти назад к -π/2; середина пути = -π/4
        assert!(approx(lerp_angle(0.0, 3.0 * FRAC_PI_2, 0.5), -FRAC_PI_4));
    }

    #[test]
    fn lerp_angle_endpoints() {
        assert!(approx(lerp_angle(0.7, 1.3, 0.0), 0.7));
        assert!(approx(lerp_angle(0.7, 1.3, 1.0), 1.3));
    }
}
