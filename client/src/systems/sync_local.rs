use bevy::prelude::*;
use crate::components::{LocalPlayer, PlayerMarker};
use crate::resources::MyPlayer;

pub fn sync_local_and_tint(
    my: Res<MyPlayer>,
    mut commands: Commands,
    mut q: Query<(Entity, &PlayerMarker, Option<&LocalPlayer>, &mut Sprite)>,
) {
    if !my.got || my.id == 0 { return; }

    for (ent, marker, has_local, mut sprite) in q.iter_mut() {
        let is_me = marker.0 == my.id;

        // поддерживаем ровно один LocalPlayer
        match (is_me, has_local.is_some()) {
            (true, false) => { commands.entity(ent).insert(LocalPlayer); }
            (false, true) => { commands.entity(ent).remove::<LocalPlayer>(); }
            _ => {}
        }

        // цвета — здесь же, чтобы не держать вторую систему
        sprite.color = if is_me {
            Color::srgba(0.0, 1.0, 0.0, 1.0) // зелёный — я
        } else {
            Color::srgba(0.0, 0.0, 1.0, 1.0) // синий — остальные
        };
    }
}