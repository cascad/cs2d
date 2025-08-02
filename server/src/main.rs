// todo solute this!
mod constants;
mod events;
mod net;
mod resources;
mod systems;
mod utils;

use bevy::{app::ctrlc, prelude::*};
use bevy_quinnet::server::{ConnectionEvent, ConnectionLostEvent, QuinnetServerPlugin};

use constants::*;
use events::*;
use resources::*;
use systems::{
    connection::*, damage::*, grenades::*, process_c2s::*,
    respawn_timers::*, server_tick::*, spawn::*, startup::*, timeout::*,
};

use crate::systems::spawn::process_player_respawn;
// use systems::{
//     connection::{handle_disconnections, handle_new_connections},
//     damage::{DamageEvent, apply_damage},
//     grenades::update_grenades,
//     process_c2s::process_c2s_messages,
//     respawn::do_respawn,
//     server_tick::server_tick,
//     startup::start_server,
//     timeout::drop_inactive,
// };

fn main() {
    // graceful Ctrl-C shutdown
    ctrlc::set_handler(|| {
        println!("⚡ Server shutting down");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl‑C handler");

    App::new()
        .insert_resource(ServerTickTimer(Timer::from_seconds(
            TICK_DT,
            TimerMode::Repeating,
        )))
        .insert_resource(PlayerStates::default())
        .insert_resource(PendingInputs::default())
        .insert_resource(AppliedSeqs::default())
        .insert_resource(LastHeard::default())
        .insert_resource(SnapshotHistory::default())
        .insert_resource(Grenades::default())
        .insert_resource(RespawnQueue::default())
        .insert_resource(RespawnDelay::default())
        .insert_resource(ConnectedClients::default())
        .insert_resource(SpawnedClients::default())
        .insert_resource(LastGrenadeThrows::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_event::<ConnectionEvent>() // регистрируем событие в ECS
        .add_event::<ConnectionLostEvent>() // регистрируем событие в ECS
        .add_event::<DamageEvent>()
        .add_event::<ClientConnected>()
        .add_event::<ClientDisconnected>()
        .add_event::<PlayerRespawn>()
        .add_systems(Startup, start_server)
        .add_systems(PreUpdate, (handle_new_connections, handle_disconnections))
        .add_systems(
            Update,
            (
                // todo !!!! сделать через ивент handler_disconnections !!!!!!
                drop_inactive,        // 1. вырубаем «молчунов»
                process_c2s_messages, // 2. обрабатываем входы (+ Heartbeat/Goodbye)
                server_tick,          // 3. рассылаем снапшот
                process_client_connected,
                process_client_disconnected,
                process_player_respawn,
                process_respawn_timers,
                apply_damage,
                update_grenades,
                // handle_player_died,
                // do_respawn,
                // purge_deaths, // todo revert???
            )
                .chain(),
        )
        .run();
}
