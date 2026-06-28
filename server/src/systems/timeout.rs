use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use crate::resources::{Accounts, AppliedSeqs, ConnectedClients, LastHeard, PendingInputs, PlayerStates, SpawnedClients};
use protocol::{constants::{CH_S2C, TIMEOUT_SECS}, messages::S2C};

#[allow(clippy::too_many_arguments)]
pub fn drop_inactive(
    time: Res<Time>,
    mut last: ResMut<LastHeard>,
    mut states: ResMut<PlayerStates>,
    mut pend: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut connected: ResMut<ConnectedClients>,
    mut spawned: ResMut<SpawnedClients>,
    mut accounts: ResMut<Accounts>,
    mut server: ResMut<QuinnetServer>,
) {
    let now = time.elapsed_secs_f64();
    let mut to_drop = Vec::new();

    for (&id, &t) in last.0.iter() {
        if now - t > TIMEOUT_SECS {
            to_drop.push(id);
        }
    }

    let mut scoreboard_dirty = false;
    for id in to_drop {
        last.0.remove(&id);
        states.0.remove(&id);
        pend.0.remove(&id);
        applied.0.remove(&id);
        connected.0.remove(&id);
        spawned.0.remove(&id);

        // ВАЖНО: освобождаем аккаунт ровно тут же. Иначе после деспавна по таймауту
        // (закрыл клиент, а quinnet ещё не заметил мёртвый сокет) аккаунт остаётся
        // `online` → реконнект ловит «Аккаунт уже в игре», пока не сработает
        // отдельный ConnectionLost. Таймаут — это тоже «выход», поэтому, как и при
        // штатном отключении, засчитываем смерть (килл никому) и отвязываем.
        if accounts.0.is_online(id) {
            accounts.0.add_death(id);
            accounts.0.unbind(id);
            scoreboard_dirty = true;
        }

        server
            .endpoint_mut()
            .broadcast_message_on(CH_S2C, S2C::PlayerLeft(id))
            .ok();
        info!("⏱ Клиент {id} бездействует >{TIMEOUT_SECS}s — очищено");
    }

    // если кого-то отвязали — разошлём обновлённую таблицу очков
    if scoreboard_dirty {
        server
            .endpoint_mut()
            .broadcast_message_on(CH_S2C, S2C::Scoreboard(accounts.0.snapshot()))
            .ok();
    }
}