use crate::resources::MyPlayer;
use bevy::prelude::*;
use bevy_quinnet::client::connection::ConnectionEvent;

pub fn handle_connection_event(mut events: EventReader<ConnectionEvent>, mut me: ResMut<MyPlayer>) {
    for ConnectionEvent { client_id, .. } in events.read() {
        if let Some(server_id) = client_id {
            if !me.got {
                me.id = *server_id;
                me.got = true;
                info!("[Client] got my id = {}", me.id);
            }
        }
    }
}
