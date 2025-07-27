use crate::components::Bullet;
use bevy::prelude::*;

pub fn bullet_lifecycle(
    mut q: Query<(Entity, &mut Transform, &mut Bullet)>,
    mut commands: Commands,
    time: Res<Time>,
) {
    for (e, mut t, mut b) in q.iter_mut() {
        b.ttl -= time.delta_secs();
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
        } else {
            t.translation += (b.vel * time.delta_secs()).extend(0.0);
        }
    }
}
