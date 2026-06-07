use crate::{
    components::{HpFill, PlayerMarker},
    events::{PlayerDamagedEvent, PlayerDied, PlayerLeftEvent},
    resources::HpUiMap,
    systems::iso::ISO_ACTOR_PX,
    systems::utils::hp_color,
};
use bevy::prelude::*;
use protocol::constants::PLAYER_MAX_HP;

/// Насколько поднять полоску HP над точкой `WorldPos` (ноги) — чуть выше макушки
/// рыцаря (спрайт ~`ISO_ACTOR_PX`, якорь на ногах).
const HP_BAR_Y_OFFSET: f32 = ISO_ACTOR_PX * 0.82;

/// Держим полоску HP строго над головой игрока (в экранных координатах).
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
                // высокий z, чтобы полоска не пряталась за спрайтами
                ui_tf.translation.z = 900.0;
            }
        }
    }
}

/// Обновляем ширину/цвет заливки HP по событию урона.
pub fn update_hp_text_from_event(
    mut evr: MessageReader<PlayerDamagedEvent>,
    mut fills: Query<(&HpFill, &mut Sprite)>,
) {
    for ev in evr.read() {
        let frac = (ev.new_hp as f32 / PLAYER_MAX_HP as f32).clamp(0.0, 1.0);
        for (fill, mut sprite) in fills.iter_mut() {
            if fill.id == ev.id {
                sprite.color = hp_color(frac);
                if let Some(sz) = sprite.custom_size.as_mut() {
                    sz.x = fill.full_w * frac;
                }
            }
        }
    }
}

pub fn cleanup_hp_ui_on_player_remove(
    mut commands: Commands,
    mut hp_ui_map: ResMut<HpUiMap>,
    mut ev_died: MessageReader<PlayerDied>,
    mut ev_left: MessageReader<PlayerLeftEvent>,
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
