use bevy::prelude::*;

use crate::components::Corpse;

pub fn corpse_lifecycle(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Corpse, &mut Sprite)>,
    mut commands: Commands,
) {
    for (ent, mut corpse, mut spr) in q.iter_mut() {
        corpse.timer.tick(time.delta());
        // мягкое затухание альфы
        let t = corpse.timer.elapsed_secs() / corpse.timer.duration().as_secs_f32();
        spr.color.set_alpha(1.0 - t.clamp(0.0, 1.0));

        if corpse.timer.finished() {
            commands.entity(ent).despawn();
        }
    }
}
