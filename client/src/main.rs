mod components;
mod constants;
mod events;
mod resources;
mod systems;

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClientPlugin;

use protocol::messages::Stance;
// Импортим общий протокол и адаптер
use protocol::constants::TICK_DT;

// Подмодули
use resources::*;
use systems::{
    bullet_lifecycle::bullet_lifecycle, connection::handle_connection_event,
    grenade_lifecycle::explosion_lifecycle, grenade_lifecycle::grenade_lifecycle,
    grenade_throw::grenade_throw, input::change_stance,
    interpolate_with_snapshot::interpolate_with_snapshot, network::receive_server_messages,
    ping::send_ping, rotate_to_cursor::rotate_to_cursor, send_input::send_input_and_predict,
    shoot::shoot_mouse, startup::setup,
};

fn main() {
    App::new()
        // ресурсы
        .insert_resource(MyPlayer { id: 0, got: false })
        .insert_resource(TimeSync { offset: 0.0 })
        .insert_resource(SnapshotBuffer {
            snapshots: VecDeque::new(),
            delay: 0.05,
        })
        .insert_resource(CurrentStance(Stance::Standing))
        .insert_resource(SendTimer(Timer::from_seconds(
            TICK_DT,
            TimerMode::Repeating,
        )))
        .insert_resource(SpawnedPlayers::default())
        .insert_resource(SeqCounter(0))
        .insert_resource(PendingInputsClient::default())
        .insert_resource(HeartbeatTimer::default())
        .insert_resource(ClientLatency::default())
        .insert_resource(DeadPlayers::default())
        // плагины
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CS‑style Multiplayer Client".into(),
                resolution: (800.0, 600.0).into(),
                ..default()
            }),
            ..default()
        }))
        // системы
        .add_plugins(QuinnetClientPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(
            PreUpdate,
            (handle_connection_event, receive_server_messages).chain(),
        )
        .add_systems(
            Update,
            (
                send_input_and_predict,
                interpolate_with_snapshot,
                bullet_lifecycle,
                grenade_lifecycle,
                explosion_lifecycle,
                grenade_throw,
                rotate_to_cursor,
                change_stance,
                shoot_mouse,
                send_ping,
            )
                .chain(),
        )
        .run();
}
