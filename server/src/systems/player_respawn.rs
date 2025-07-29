use crate::{
    events::PlayerRespawn,
    resources::{PlayerState, PlayerStates, SpawnedClients},
};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::CH_S2C,
    messages::{S2C, Stance},
};

/// Реагируем на внутреннее событие PlayerRespawn:
/// 1) обновляем PlayerStates + SpawnedClients
/// 2) рассылаем пакет S2C::PlayerRespawn всем клиентам
pub fn process_player_respawn(
    mut ev: EventReader<PlayerRespawn>,
    mut states: ResMut<PlayerStates>,
    mut spawned: ResMut<SpawnedClients>,
    mut server: ResMut<QuinnetServer>,
) {
    for PlayerRespawn { id, x, y } in ev.read() {
        // 1) вставляем или обновляем состояние
        let pos = Vec2::new(*x, *y);
        states.0.insert(
            *id,
            PlayerState {
                pos,
                rot: 0.0,
                stance: Stance::Standing,
                hp: 100,
            },
        );
        // 2) помечаем заспавненным
        spawned.0.insert(*id);
        // 3) broadcast
        server
            .endpoint_mut()
            .broadcast_message_on(
                CH_S2C,
                S2C::PlayerRespawn {
                    id: *id,
                    x: *x,
                    y: *y,
                },
            )
            .unwrap();
        info!(
            "[Server] → PlayerRespawn {{ id: {}, x: {:.1}, y: {:.1} }}",
            id, x, y
        );
    }
}
