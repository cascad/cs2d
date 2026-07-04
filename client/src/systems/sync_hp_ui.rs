use crate::{
    components::PlayerMarker,
    resources::HpUiMap,
    systems::iso::ISO_ACTOR_PX,
};
use bevy::prelude::*;

const HP_BAR_Y_OFFSET: f32 = ISO_ACTOR_PX * 0.82;

pub fn sync_hp_ui_position(
    player_query: Query<(&Transform, &PlayerMarker), With<PlayerMarker>>,
    hp_ui_map: Res<HpUiMap>,
    mut ui_tf_query: Query<&mut Transform, Without<PlayerMarker>>,
) {
    for (player_tf, marker) in player_query.iter() {
        if let Some(&ui_ent) = hp_ui_map.0.get(&marker.0) {
            if let Ok(mut ui_tf) = ui_tf_query.get_mut(ui_ent) {
                ui_tf.translation.x = player_tf.translation.x;
                ui_tf.translation.y = player_tf.translation.y + HP_BAR_Y_OFFSET;
                ui_tf.translation.z = 900.0;
            }
        }
    }
}

// ширину/цвет заливки теперь ведёт `lynet::sync_hp_bars` напрямую из свежего
// реплицируемого HP (событие урона ловит только ПАДЕНИЕ HP — после респауна
// полоска оставалась пустой)

/// Убираем полоски HP игроков, чьи сущности исчезли (отключение / туман войны).
pub fn cleanup_hp_ui_on_player_remove(
    mut commands: Commands,
    mut hp_ui_map: ResMut<HpUiMap>,
    players: Query<&PlayerMarker>,
) {
    let alive: std::collections::HashSet<u64> = players.iter().map(|m| m.0).collect();
    hp_ui_map.0.retain(|id, ent| {
        if alive.contains(id) {
            true
        } else {
            commands.entity(*ent).despawn();
            false
        }
    });
}
