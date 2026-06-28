use crate::{
    events::{ClientConnected, ClientDisconnected, PlayerRespawn},
    resources::{Accounts, ConnectedClients, PlayerStates, SpawnPoints, SpawnedClients},
};
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::constants::CH_S2C;
use protocol::messages::S2C;

// todo загружать логику спавнов отдельно
/// Функция, где вы решаете стартовую позицию
/// Простая функция, возвращающая точку спавна по ID.
/// Замените логику на свою: рандом, круг, свободные точки и т.д.
// fn pick_spawn_point(pid: u64) -> Vec2 {
//     const POINTS: [Vec2; 4] = [
//         Vec2::new(-300.0, -200.0),
//         Vec2::new(300.0, -200.0),
//         Vec2::new(-300.0, 200.0),
//         Vec2::new(300.0, 200.0),
//     ];
//     let idx = (pid as usize) % POINTS.len();
//     POINTS[idx]
// }


// todo сделать рандом тут
pub fn pick_spawn_point(spawns: &SpawnPoints, index_hint: u64) -> Vec2 {
    if spawns.0.is_empty() {
        return Vec2::ZERO;
    }
    let i = (index_hint as usize) % spawns.0.len();
    spawns.0[i]
}

/// Транспорт сообщил о новом соединении. Спавн НЕ делаем здесь — игрок появится
/// только после успешной авторизации (`C2S::Hello`). Тут лишь отмечаем факт
/// подключения, чтобы таймауты/учёт соединений видели клиента.
pub fn process_client_connected(
    mut ev: MessageReader<ClientConnected>,
    mut connected: ResMut<ConnectedClients>,
) {
    for ClientConnected(id) in ev.read() {
        connected.0.insert(*id);
        info!("🔌 Соединение {id} установлено, ждём авторизацию");
    }
}

pub fn process_client_disconnected(
    mut ev: MessageReader<ClientDisconnected>,
    mut connected: ResMut<ConnectedClients>,
    mut spawned: ResMut<SpawnedClients>,
    mut states: ResMut<PlayerStates>,
    mut accounts: ResMut<Accounts>,
    mut server: ResMut<QuinnetServer>,
) {
    for ClientDisconnected(id) in ev.read() {
        connected.0.remove(id);
        spawned.0.remove(id);
        states.0.remove(id);

        // Выход из игры засчитываем как СМЕРТЬ (килл никому — его никто не убил),
        // затем отвязываем соединение, сохраняя статистику аккаунта. Гард
        // `is_online` не даёт задвоить смерть, если придёт ещё одно событие.
        let mut changed = false;
        if accounts.0.is_online(*id) {
            accounts.0.add_death(*id);
            accounts.0.unbind(*id);
            changed = true;
        }

        let endpoint = server.endpoint_mut();
        if let Err(e) =
            endpoint.broadcast_message_on(CH_S2C, S2C::PlayerDisconnected { id: *id })
        {
            warn!("broadcast PlayerDisconnected failed: {e:?}");
        }
        if changed {
            let _ = endpoint
                .broadcast_message_on(CH_S2C, S2C::Scoreboard(accounts.0.snapshot()));
        }
    }
}

pub fn process_player_respawn(
    mut ev: MessageReader<PlayerRespawn>,
    mut spawned: ResMut<SpawnedClients>,
    mut states: ResMut<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
) {
    for PlayerRespawn { id, x, y } in ev.read() {
        spawned.0.insert(*id);
        let st = states.0.entry(*id).or_default();
        st.pos = Vec2::new(*x, *y);
        st.hp = 100;

        if let Err(e) = server.endpoint_mut().broadcast_message_on(
            CH_S2C,
            S2C::PlayerRespawn {
                id: *id,
                x: *x,
                y: *y,
            },
        ) {
            warn!("broadcast PlayerRespawn failed: {e:?}");
        }
    }
}
