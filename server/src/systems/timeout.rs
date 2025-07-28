use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use crate::resources::{LastHeard, PlayerStates, PendingInputs, AppliedSeqs};
use protocol::{constants::{CH_S2C, TIMEOUT_SECS}, messages::S2C};

pub fn drop_inactive(
    time: Res<Time>,
    mut last: ResMut<LastHeard>,
    mut states: ResMut<PlayerStates>,
    mut pend: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut server: ResMut<QuinnetServer>,
) {
    let now = time.elapsed_secs_f64();
    let mut to_drop = Vec::new();

    for (&id, &t) in last.0.iter() {
        if now - t > TIMEOUT_SECS {
            to_drop.push(id);
        }
    }

    for id in to_drop {
        last.0.remove(&id);
        states.0.remove(&id);
        pend.0.remove(&id);
        applied.0.remove(&id);

        server
            .endpoint_mut()
            .broadcast_message_on(CH_S2C, S2C::PlayerLeft(id))
            .ok();
        info!("⏱ Клиент {id} бездействует >{TIMEOUT_SECS}s — очищено");
    }
}