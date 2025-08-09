use bevy::prelude::*;

use crate::{
    components::{LocalPlayer, PlayerMarker},
    resources::MyPlayer,
};

pub fn debug_player_colors_on_added(
    q: Query<(&Name, &PlayerMarker, Option<&LocalPlayer>, &Sprite), Added<PlayerMarker>>,
) {
    for (name, marker, is_local, spr) in &q {
        let c = spr.color.to_srgba();
        info!(
            "🎨 ADDED {} id={} local={} color=({:.3},{:.3},{:.3},{:.3})",
            name.as_str(), marker.0, is_local.is_some(),
            c.red, c.green, c.blue, c.alpha
        );
    }
}

/// После сетевых апдейтов гарантирует:
/// - LocalPlayer висит только на сущности с PlayerMarker(my.id)
/// - цвет: свой — зелёный, чужие — синие
pub fn reconcile_local_and_colors(
    my: Res<MyPlayer>,
    mut commands: Commands,
    mut q: Query<(Entity, &PlayerMarker, Option<&LocalPlayer>, &mut Sprite)>,
) {
    if my.id == 0 { return; } // id ещё не получили

    for (e, marker, has_local, mut spr) in q.iter_mut() {
        if marker.0 == my.id {
            if has_local.is_none() {
                commands.entity(e).insert(LocalPlayer);
                // dbg: info!("attach LocalPlayer -> {}", marker.0);
            }
            // свой = зелёный
            let want = Color::srgba(0.0, 1.0, 0.0, 1.0);
            if spr.color != want { spr.color = want; }
        } else {
            if has_local.is_some() {
                commands.entity(e).remove::<LocalPlayer>();
                // dbg: info!("remove LocalPlayer from {}", marker.0);
            }
            // чужой = синий
            let want = Color::srgba(0.0, 0.0, 1.0, 1.0);
            if spr.color != want { spr.color = want; }
        }
    }
}