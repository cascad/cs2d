// todo solute this!
mod config;
mod constants;
mod events;
mod net;
mod resources;
mod systems;
mod utils;

use std::time::Duration;

use bevy::{
    app::{ctrlc, ScheduleRunnerPlugin},
    log::{Level, LogPlugin},
    prelude::*,
};
use bevy_quinnet::server::{ConnectionEvent, ConnectionLostEvent, QuinnetServerPlugin};

use constants::*;
use events::*;
use resources::*;
use systems::{
    connection::*, damage::*, npc::*, process_c2s::*, respawn_timers::*, server_tick::*, spawn::*,
    startup::*, timeout::*, update_grenades::*,
};

use crate::systems::{level_fixed::setup_fixed_level, spawn::process_player_respawn};
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
        // конфиг (ip/port) из файла рядом с бинарём — создаётся при первом запуске
        .insert_resource(config::load_or_create())
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
        .insert_resource(GrenadeSyncTimer(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        ))) // 10 Гц
        .insert_resource(Npcs::default())
        .insert_resource(NpcIdCounter::default())
        .insert_resource(NpcRoutes::default())
        .insert_resource(NpcSpawnPoints::default())
        .insert_resource(NpcRespawnTimer::default())
        .insert_resource(PendingMelees::default())
        .insert_resource(Reveals::default())
        .add_plugins((
            // Ограничиваем главный цикл частотой тика (64 Гц).
            // Иначе ScheduleRunnerPlugin по умолчанию крутит цикл без сна и жрёт ядро на 100%.
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f32(
                TICK_DT,
            ))),
            LogPlugin {
                // лог-плагин отдельно
                level: Level::INFO,           // показываем info
                filter: "server=info".into(), // или "" чтобы видеть всё
                ..Default::default()
            },
        ))
        .add_plugins(QuinnetServerPlugin::default())
        .add_message::<ConnectionEvent>() // регистрируем сообщение в ECS
        .add_message::<ConnectionLostEvent>() // регистрируем сообщение в ECS
        .add_message::<DamageEvent>()
        .add_message::<NpcDamageEvent>()
        .add_message::<ClientConnected>()
        .add_message::<ClientDisconnected>()
        .add_message::<PlayerRespawn>()
        .add_systems(Startup, (start_server, setup_fixed_level, setup_npcs).chain()) // spawn_level_server
        .add_systems(PreUpdate, (handle_new_connections, handle_disconnections))
        .add_systems(
            Update,
            (
                // todo !!!! сделать через ивент handler_disconnections !!!!!!
                drop_inactive,        // 1. вырубаем «молчунов»
                process_c2s_messages, // 2. обрабатываем входы (+ Heartbeat/Goodbye)
                resolve_melees,       // 2.4 урон ближнего боя в середине взмаха
                npc_ai,               // 2.5 ИИ скелетов (до снапшота)
                server_tick,          // 3. рассылаем снапшот
                process_client_connected,
                process_client_disconnected,
                process_player_respawn,
                process_respawn_timers,
                apply_damage,
                update_grenades,
                broadcast_grenade_syncs,
                update_meta_player_count,
                // handle_player_died,
                // do_respawn,
                // purge_deaths, // todo revert???
            )
                .chain(),
        )
        .run();
}
