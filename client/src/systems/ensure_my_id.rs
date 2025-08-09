// systems/ensure_local.rs
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use crate::resources::MyPlayer;

pub fn ensure_my_id_from_conn(
    mut my: ResMut<MyPlayer>,
    client: ResMut<QuinnetClient>,
) {
    if my.id != 0 {
        return;
    }
    if let Some(conn) = client.get_connection() {
        if let Some(cid) = conn.client_id() {
            my.id = cid;
            info!("✅ MyPlayer.id set to {}", my.id);
        }
    }
}