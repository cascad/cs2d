use bevy::prelude::*;
use bevy_quinnet::server::{QuinnetServer, ConnectionLostEvent};

use crate::resources::{PlayerStates, PendingInputs, AppliedSeqs};
use protocol::messages::S2C;
use protocol::constants::CH_S2C;

/// Вызывается в PreUpdate.  Удаляет состояние всех клиентов,
/// по которым пришёл ConnectionLostEvent, и тут же сообщает
/// оставшимся клиентам `S2C::PlayerLeft(id)`.
pub fn purge_disconnected(
    mut events : EventReader<ConnectionLostEvent>,
    mut states : ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut server : ResMut<QuinnetServer>,           // ← добавили
) {
    for ConnectionLostEvent { id } in events.read() {
        // 1. Удаляем все следы клиента
        states.0.remove(id);
        pending.0.remove(id);
        applied.0.remove(id);
        info!("❌ Клиент {id} LOST — состояние очищено");

        // 2. Сообщаем остальным
        server
            .endpoint_mut()
            .broadcast_message_on(CH_S2C, S2C::PlayerLeft(*id))
            .ok();
    }
}
