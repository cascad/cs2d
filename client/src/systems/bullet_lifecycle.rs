use crate::components::Bullet;
use bevy::prelude::*;

pub fn bullet_lifecycle(
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &mut Bullet)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut b) in q.iter_mut() {
        b.ttl -= dt;
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
        } else {
            tf.translation += (b.vel * dt).extend(0.0);
        }
    }
}