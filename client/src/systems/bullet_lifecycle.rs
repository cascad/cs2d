use crate::components::Bullet;
use crate::render::WorldPos;
use bevy::prelude::*;

pub fn bullet_lifecycle(
    mut commands: Commands,
    mut q: Query<(Entity, &mut WorldPos, &mut Bullet)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (e, mut wp, mut b) in q.iter_mut() {
        b.ttl -= dt;
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
        } else {
            wp.0 += b.vel * dt;
        }
    }
}
