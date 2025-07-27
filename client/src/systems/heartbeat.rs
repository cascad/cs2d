use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::CH_C2S;
use protocol::messages::C2S;
use crate::resources::HeartbeatTimer;

pub fn send_heartbeat(
    time:    Res<Time>,
    mut timer: ResMut<HeartbeatTimer>,
    mut client: ResMut<QuinnetClient>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        // просто отправляем пустой Heartbeat
        let _ = client.connection_mut().send_message_on(CH_C2S, C2S::Heartbeat);
        info!("💓 Sent heartbeat");
    }
}