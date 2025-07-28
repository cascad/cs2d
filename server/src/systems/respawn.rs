use crate::resources::{PlayerState, PlayerStates, RespawnQueue};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{constants::CH_S2C, messages::{Stance, S2C}};

pub fn do_respawn(
    mut respawn_q: ResMut<RespawnQueue>,
    mut states:   ResMut<PlayerStates>,
    mut server:   ResMut<QuinnetServer>,
    time:         Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    respawn_q.0.retain(|task| {
        if now >= task.due {
            info!(
                "[Server] → PlayerRespawn {{ id: {}, x: {}, y: {} }} at t={}",
                task.pid, task.pos.x, task.pos.y, now
            );
            // 1) шлём PlayerRespawn заранее, до server_tick
            server.endpoint_mut()
                .broadcast_message_on(
                    CH_S2C,
                    S2C::PlayerRespawn {
                        id: task.pid,
                        x:  task.pos.x,
                        y:  task.pos.y,
                    },
                )
                .unwrap();

            // 2) заносим игрока в состояние, чтобы следующий server_tick включил его
            states.0.insert(task.pid, PlayerState {
                pos:    task.pos,
                rot:    0.0,
                stance: Stance::Standing,
                hp:     100,
            });

            info!("🔄 Player {} respawned at ({}, {})", task.pid, task.pos.x, task.pos.y);
            false
        } else {
            true
        }
    });
}
