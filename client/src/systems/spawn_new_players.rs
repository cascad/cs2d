use crate::components::PlayerMarker;
use crate::resources::{MyPlayer, SnapshotBuffer, SpawnedPlayers};
use bevy::prelude::*;

pub fn spawn_new_players(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    mut spawned: ResMut<SpawnedPlayers>,
    my: Res<MyPlayer>,
) {
    if let Some(last) = buffer.snapshots.back() {
        for p in &last.players {
            if p.id == my.id {
                continue;
            }
            if spawned.0.insert(p.id) {
                commands.spawn((
                    Sprite {
                        color: Color::srgb(0.2, 0.4, 1.0),
                        custom_size: Some(Vec2::splat(40.0)),
                        ..default()
                    },
                    Transform::from_xyz(p.x, p.y, 0.0)
                        .with_rotation(Quat::from_rotation_z(p.rotation)),
                    GlobalTransform::default(),
                    PlayerMarker(p.id),
                ));
            }
        }
    }
}
