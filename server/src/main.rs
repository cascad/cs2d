// todo solute this!
mod constants;
mod net;
mod resources;
mod systems;
mod utils;

use bevy::{app::ctrlc, prelude::*};
use bevy_quinnet::server::{ConnectionLostEvent, QuinnetServer, QuinnetServerPlugin};

use constants::*;
use resources::*;
use systems::{
    disconnect::purge_disconnected, process_c2s::process_c2s_messages, server_tick::server_tick,
    startup::start_server, timeout::drop_inactive,
};

fn main() {
    // graceful Ctrl-C shutdown
    ctrlc::set_handler(|| {
        println!("⚡ Server shutting down");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl‑C handler");

    App::new()
        .insert_resource(PlayerStates::default())
        .insert_resource(PendingInputs::default())
        .insert_resource(AppliedSeqs::default())
        .insert_resource(LastHeard::default())
        .insert_resource(ServerTickTimer(Timer::from_seconds(
            TICK_DT,
            TimerMode::Repeating,
        )))
        .insert_resource(SnapshotHistory::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_event::<ConnectionLostEvent>() // регистрируем событие в ECS
        // 1)  сначала, ещё в PreUpdate, убираем всех отключившихся
        // todo revert
        // .add_systems(PreUpdate, (handle_client_disconnected, monitor_connections))
        .add_systems(Startup, start_server)
        .add_systems(
            Update,
            (
                drop_inactive,        // 1. вырубаем «молчунов»
                process_c2s_messages, // 2. обрабатываем входы (+ Heartbeat/Goodbye)
                server_tick,          // 3. рассылаем снапшот
            )
                .chain(),
        )
        .run();
}

// fn handle_client_connected(
//     mut events: EventReader<ClientConnectedEvent>,
//     mut game_state: ResMut<GameState>,
// ) {
//     for event in events.read() {
//         let client_id = event.client_id;
//         let now = Instant::now();

//         let player = PlayerData {
//             client_id,
//             name: format!("Player_{}", client_id),
//             position: Vec3::ZERO,
//             health: 100.0,
//         };

//         game_state.players.insert(client_id, player);
//         game_state.last_heartbeat.insert(client_id, now);

//         info!("Игрок {} подключился", client_id);
//     }
// }

// // Автоматическая очистка при отключении
// fn handle_client_disconnected(
//     mut events: EventReader<ConnectionLostEvent>,
//     mut game_state: ResMut<PlayerStates>,
// ) {
//     for event in events.read() {
//         let client_id = event.id;

//         // Просто удаляем игрока из состояния
//         if let Some(_player) = game_state.0.remove(&client_id) {
//             info!(
//                 "Игрок {} отключился и удален (осталось игроков: {})",
//                 client_id,
//                 game_state.0.len()
//             );
//         }
//     }
// }

// // Опционально: система для мониторинга активных подключений
// fn monitor_connections(server: Res<QuinnetServer>, game_state: Res<PlayerStates>) {
//     let endpoint = server.endpoint();
//     let connected_clients: Vec<u64> = endpoint.clients().into_iter().collect();
//     println!("conn clients: {:?}", connected_clients);

//     // Проверяем, что состояние игры синхронизировано с реальными подключениями
//     for client_id in connected_clients {
//         if !game_state.0.contains_key(&client_id) {
//             warn!(
//                 "Клиент {} подключен, но отсутствует в состоянии игры",
//                 client_id
//             );
//         }
//     }

//     // Проверяем на "призрачных" игроков в состоянии
//     for player_id in game_state.0.keys() {
//         if !endpoint.clients().contains(player_id) {
//             warn!("Игрок {} в состоянии, но не подключен", player_id);
//         }
//     }
// }
