// todo solute this!
mod constants;
mod net;
mod resources;
mod systems;
mod utils;

use bevy::{app::ctrlc, prelude::*};
use bevy_quinnet::server::{ConnectionEvent, ConnectionLostEvent, QuinnetServerPlugin};

use constants::*;
use resources::*;
use systems::{
    connection::handle_connection_events,
    damage::{DamageEvent, apply_damage},
    grenades::update_grenades,
    process_c2s::process_c2s_messages,
    respawn::do_respawn,
    server_tick::server_tick,
    startup::start_server,
    timeout::drop_inactive,
};

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
        .insert_resource(resources::PlayerStates::default())
        .insert_resource(resources::RespawnQueue::default())
        .insert_resource(resources::RespawnDelay::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_event::<ConnectionEvent>() // регистрируем событие в ECS
        .add_event::<ConnectionLostEvent>() // регистрируем событие в ECS
        .add_event::<DamageEvent>()
        .add_systems(Startup, start_server)
        .add_systems(
            Update,
            (
                handle_connection_events.before(process_c2s_messages),
                drop_inactive,        // 1. вырубаем «молчунов»
                process_c2s_messages, // 2. обрабатываем входы (+ Heartbeat/Goodbye)
                apply_damage,
                do_respawn,
                update_grenades,
                // purge_deaths, // todo revert???
                server_tick, // 3. рассылаем снапшот
            )
                .chain(),
        )
        .run();
}
