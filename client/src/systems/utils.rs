use bevy::prelude::*;

pub fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    now.as_secs_f64()
}

pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}

pub fn spawn_hp_ui(commands: &mut Commands, player_id: u64, hp: u32, font: Handle<Font>) -> Entity {
    commands
        .spawn((
            Text2d(format!("{} HP", hp)),
            TextFont {
                font: font.into(),
                font_size: 14.0,
                ..Default::default()
            },
            TextColor(Color::WHITE.into()),
        ))
        .id()
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
