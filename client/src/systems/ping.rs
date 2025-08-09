use crate::resources::ClientLatency;
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::{constants::CH_C2S, messages::C2S}; // или ваша функция для UNIX‑времени

pub fn send_ping(
    time: Res<Time>,
    mut latency: ResMut<ClientLatency>,
    mut client: ResMut<QuinnetClient>,
) {
    // тикнем единственный таймер
    if latency.timer.tick(time.delta()).just_finished() {
        let ts = time_in_seconds();
        if client
            .connection_mut()
            .send_message_on(CH_C2S, C2S::Ping(ts))
            .is_ok()
        {
            // info!("💓 Sent Ping at {:.3}", ts);
        }
    }
}
