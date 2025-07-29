use crate::events::ClientConnected;
use crate::events::ClientDisconnected;
use bevy::prelude::*;
use bevy_quinnet::server::{ConnectionEvent, ConnectionLostEvent};

/// Просто переводим низкоуровневые события плагина в наши ECS‑события
pub fn handle_new_connections(
    mut ev_q: EventReader<ConnectionEvent>,
    mut out: EventWriter<ClientConnected>,
) {
    for ConnectionEvent { id } in ev_q.read() {
        out.write(ClientConnected(*id));
    }
}

pub fn handle_disconnections(
    mut ev_q: EventReader<ConnectionLostEvent>,
    mut out: EventWriter<ClientDisconnected>,
) {
    for ConnectionLostEvent { id } in ev_q.read() {
        out.write(ClientDisconnected(*id));
    }
}
