use crate::{
    events::{ClientConnected, ClientDisconnected, PlayerRespawn},
    resources::{ConnectedClients, PlayerState, PlayerStates, SpawnPoints, SpawnedClients},
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
fn pick_spawn_point(spawns: &Res<SpawnPoints>, index_hint: u64) -> Vec2 {
    if spawns.0.is_empty() {
        return Vec2::ZERO;
    }
    let i = (index_hint as usize) % spawns.0.len();
    spawns.0[i]
}

pub fn process_client_connected(
    mut ev: EventReader<ClientConnected>,
    mut connected: ResMut<ConnectedClients>,
    mut spawned: ResMut<SpawnedClients>,
    mut states: ResMut<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
    spawns: Res<SpawnPoints>,
) {
    for ClientConnected(id) in ev.read() {
        if !connected.0.insert(*id) {
            continue;
        }
        let pos = pick_spawn_point(&spawns, *id);
        states.0.insert(
            *id,
            PlayerState {
                pos,
                rot: 0.0,
                stance: Default::default(),
                hp: 100,
            },
        );
        spawned.0.insert(*id);

        server
            .endpoint_mut()
            .broadcast_message_on(
                CH_S2C,
                S2C::PlayerConnected {
                    id: *id,
                    x: pos.x,
                    y: pos.y,
                },
            )
            .unwrap();
    }
}

pub fn process_client_disconnected(
    mut ev: EventReader<ClientDisconnected>,
    mut connected: ResMut<ConnectedClients>,
    mut spawned: ResMut<SpawnedClients>,
    mut states: ResMut<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
) {
    for ClientDisconnected(id) in ev.read() {
        connected.0.remove(id);
        spawned.0.remove(id);
        states.0.remove(id);

        server
            .endpoint_mut()
            .broadcast_message_on(CH_S2C, S2C::PlayerDisconnected { id: *id })
            .unwrap();
    }
}

pub fn process_player_respawn(
    mut ev: EventReader<PlayerRespawn>,
    mut spawned: ResMut<SpawnedClients>,
    mut states: ResMut<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
) {
    for PlayerRespawn { id, x, y } in ev.read() {
        spawned.0.insert(*id);
        let st = states.0.entry(*id).or_default();
        st.pos = Vec2::new(*x, *y);
        st.hp = 100;

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
    }
}
