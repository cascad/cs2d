pub mod server {
    pub mod connection;
    pub mod processing;
    pub mod systems;

    pub use connection::*;
    pub use processing::*;
    pub use systems::*;
}

use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServerPlugin;
use server::start_server;
// не могу понять как принято
// use server::processing::{PlayerStates, PendingInputs};

use crate::server::{
    process_c2s_messages, server_tick, set_server_tick_timer, AppliedSeqs, PendingInputs,
    PlayerStates, SnapshotHistory,
};

const TICK_DT: f32 = 0.015;

fn main() {
    // Ctrl‑C → graceful exit
    ctrlc::set_handler(move || {
        println!("⚡ Server shutting down");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl‑C handler");

    App::new()
        .insert_resource(PlayerStates::default())
        .insert_resource(PendingInputs::default())
        .insert_resource(AppliedSeqs::default())
        .insert_resource(set_server_tick_timer(TICK_DT))
        .insert_resource(SnapshotHistory::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_systems(Startup, start_server)
        .add_systems(Update, (process_c2s_messages, server_tick).chain())
        .run();
}
