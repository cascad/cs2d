use crate::resources::MyPlayer;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;

pub fn grab_my_id(client: Res<QuinnetClient>, mut me: ResMut<MyPlayer>) {
    println!("grab_my_id here!!!!!!");
    if me.got {
        return;
    }
    if let Some(id) = client.connection().client_id() {
        me.id = id;
        me.got = true;
        info!("[Client] got my id = {}", id);
    }
}
