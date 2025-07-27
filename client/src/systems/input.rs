use bevy::prelude::*;
use crate::resources::{CurrentStance};
use protocol::messages::Stance;

pub fn change_stance(keys: Res<ButtonInput<KeyCode>>, mut stance: ResMut<CurrentStance>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Crouching,
            Stance::Crouching => Stance::Prone,
            Stance::Prone => Stance::Standing,
        };
    } else if keys.just_pressed(KeyCode::KeyE) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Prone,
            Stance::Prone => Stance::Crouching,
            Stance::Crouching => Stance::Standing,
        };
    }
}