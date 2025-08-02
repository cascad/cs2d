use bevy::prelude::*;

use crate::resources::grenades::GrenadeCooldown;


#[derive(Component)]
pub struct GrenadeCooldownBar;

pub fn update_grenade_cooldown_ui(
    grenade_cd: Res<GrenadeCooldown>,
    mut query: Query<&mut Node, With<GrenadeCooldownBar>>,
) {
    if !grenade_cd.is_changed() {
        return;
    }

    let elapsed = grenade_cd.0.elapsed().as_secs_f32();
    let duration = grenade_cd.0.duration().as_secs_f32();
    let percent = if grenade_cd.0.finished() {
        1.0
    } else {
        1.0 - (elapsed / duration)
    };

    for mut node in &mut query {
        node.width = Val::Percent(percent * 100.0);
    }
}