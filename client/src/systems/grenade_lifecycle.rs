use bevy::prelude::*;
use crate::components::GrenadeTimer;

pub fn grenade_lifecycle(
    mut commands: Commands,
    time:       Res<Time>,
    mut query:  Query<(Entity, &mut GrenadeTimer)>,
) {
    for (entity, mut gt) in query.iter_mut() {
        gt.0.tick(time.delta());
        if gt.0.finished() {
            commands.entity(entity).despawn();
            info!("💥 Grenade {:?} exploded and despawned", entity);
        }
    }
}