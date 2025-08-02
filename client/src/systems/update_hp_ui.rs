use bevy::prelude::*;

use crate::{resources::SnapshotBuffer, ui::components::PlayerHpUi};

pub fn update_hp_ui(buffer: Res<SnapshotBuffer>, mut q_text: Query<(&PlayerHpUi, &mut Text2d)>) {
    if let Some(snap) = buffer.snapshots.back() {
        for (hp_ui, mut text) in q_text.iter_mut() {
            if let Some(player) = snap.players.iter().find(|p| p.id == hp_ui.player_id) {
                text.0 = format!("HP: {}", player.hp);
            }
        }
    }
}
