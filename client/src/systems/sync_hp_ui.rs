use crate::{
    components::PlayerMarker,
    events::{PlayerDamagedEvent, PlayerDied, PlayerLeftEvent},
    resources::HpUiMap,
};
use bevy::prelude::*;

pub fn sync_hp_ui_position(
    player_query: Query<(&Transform, &PlayerMarker), With<PlayerMarker>>,
    mut hp_ui_map: ResMut<HpUiMap>,
    mut ui_tf_query: Query<&mut Transform, Without<PlayerMarker>>,
) {
    for (player_tf, marker) in player_query.iter() {
        if let Some(&ui_ent) = hp_ui_map.0.get(&marker.0) {
            if let Ok(mut ui_tf) = ui_tf_query.get_mut(ui_ent) {
                ui_tf.translation.x = player_tf.translation.x;
                ui_tf.translation.y = player_tf.translation.y + 32.0;
            }
        }
    }
}

pub fn update_hp_text_from_event(
    mut evr: EventReader<PlayerDamagedEvent>,
    hp_ui_map: Res<HpUiMap>,
    mut text_query: Query<&mut Text2d>,
) {
    for ev in evr.read() {
        if let Some(&ui_ent) = hp_ui_map.0.get(&ev.id) {
            if let Ok(mut text2d) = text_query.get_mut(ui_ent) {
                text2d.0 = format!("{} HP", ev.new_hp);
            }
        }
    }
}

pub fn cleanup_hp_ui_on_player_remove(
    mut commands: Commands,
    mut hp_ui_map: ResMut<HpUiMap>,
    mut ev_died: EventReader<PlayerDied>,
    mut ev_left: EventReader<PlayerLeftEvent>,
) {
    for ev in ev_died.read() {
        if let Some(ent) = hp_ui_map.0.remove(&ev.victim) {
            commands.entity(ent).despawn();
        }
    }
    for ev in ev_left.read() {
        if let Some(ent) = hp_ui_map.0.remove(&ev.0) {
            commands.entity(ent).despawn();
        }
    }
}
