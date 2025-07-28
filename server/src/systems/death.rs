use crate::resources::{AppliedSeqs, PendingInputs, PlayerStates};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{CH_S2C, RESPAWN_COOLDOWN},
    messages::S2C,
};

pub fn purge_deaths(
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut server: ResMut<QuinnetServer>,
    time: Res<Time>,
    mut respawn_q: Local<Vec<(u64, f64)>>, // (player_id, respawn_time)
) {
    let now = time.elapsed_secs_f64();
    // здесь respawn_q наполняется из update_grenades при hp<=0

    // по таймеру респауним
    for &(pid, t0) in respawn_q.iter() {
        if now - t0 >= RESPAWN_COOLDOWN {
            // респаун через RESPAWN_COOLDOWN с
            // сбрасываем состояние
            states.0.insert(pid, Default::default());
            // шлём всем клиентам событие респауна
            server
                .endpoint_mut()
                .broadcast_message_on(
                    CH_S2C,
                    S2C::PlayerRespawn {
                        id: pid,
                        x: 0.0,
                        y: 0.0,
                    },
                )
                .ok();
        }
    }
    respawn_q.retain(|&(_, t0)| now - t0 < 5.0);
}
