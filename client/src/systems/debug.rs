use bevy::prelude::*;
use crate::components::PlayerMarker;

pub fn debug_player_spawn(
    mut q: Query<(&PlayerMarker, &Transform), Added<PlayerMarker>>,
    time: Res<Time>,
) {
    for (marker, tf) in q.iter_mut() {
        info!(
            "[Debug] Spawned Player {} at {:?} (t={:.3})",
            marker.0,
            tf.translation,
            time.elapsed_secs_f64(),
        );
    }
}
