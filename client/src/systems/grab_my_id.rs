use crate::components::{LocalPlayer, PlayerMarker};
use crate::resources::{MyPlayer};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;

pub fn grab_my_id(client: Res<QuinnetClient>, mut me: ResMut<MyPlayer>, mut commands: Commands) {
    if me.got {
        return;
    }
    if let Some(id) = client.connection().client_id() {
        me.id = id;
        me.got = true;
        commands.spawn((
            Sprite {
                color: Color::srgb(0.0, 1.0, 0.0),
                custom_size: Some(Vec2::splat(40.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
            GlobalTransform::default(),
            PlayerMarker(me.id),
            LocalPlayer,
        ));
    }
}
