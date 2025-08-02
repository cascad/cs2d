use crate::{components::PlayerMarker, events::PlayerDamagedEvent, ui::components::PlayerHpUi};
use bevy::prelude::*;

pub fn sync_hp_ui_position(
    player_query: Query<(&Transform, &PlayerMarker), Without<PlayerHpUi>>,
    mut ui_query: Query<(&mut Transform, &PlayerHpUi)>,
) {
    // Кэшируем позиции всех игроков
    let mut player_positions = Vec::new();
    for (tf, marker) in player_query.iter() {
        player_positions.push((marker.0, tf.translation));
    }

    // Обновляем позиции UI
    for (mut ui_tf, hp_ui) in ui_query.iter_mut() {
        if let Some((_, player_pos)) = player_positions
            .iter()
            .find(|(id, _)| *id == hp_ui.player_id)
        {
            ui_tf.translation.x = player_pos.x;
            ui_tf.translation.y = player_pos.y + 32.0;
        }
    }
}

pub fn update_hp_text_from_event(
    mut evr: EventReader<PlayerDamagedEvent>,
    mut query: Query<(&PlayerHpUi, &mut Text2d)>, // или другой компонент, если не Text2d
) {
    for ev in evr.read() {
        for (hp_ui, mut text) in query.iter_mut() {
            if hp_ui.player_id == ev.id {
                text.0 = format!("{} HP", ev.new_hp);
            }
        }
    }
}
