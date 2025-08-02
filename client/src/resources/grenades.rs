use bevy::prelude::*;

#[derive(Resource)]
pub struct GrenadeCooldown(pub Timer);

impl Default for GrenadeCooldown {
    fn default() -> Self {
        GrenadeCooldown(Timer::from_seconds(2.0, TimerMode::Once))
    }
}

// todo move here all about grenades from mod.rs