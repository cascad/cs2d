mod client;

use client::{ClientConnectionPlugin, grab_my_id, rotate_to_cursor, send_input_and_predict};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClientPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(QuinnetClientPlugin::default())
        .add_startup_system(grab_my_id.system())
        .add_startup_system(rotate_to_cursor.system())
        .add_startup_system(send_input_and_predict.system())
        .add_plugin(ClientConnectionPlugin)  // Подключение плагина для клиентского соединения
        .run();
}

fn main() {
    App::new()
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
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CS-style Multiplayer Client".into(),
                resolution: (800.0, 600.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(QuinnetClientPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                grab_my_id,
                rotate_to_cursor,
                shoot_mouse,
                change_stance,
                send_input_and_predict,
                receive_server_messages,
                spawn_new_players,
                interpolate_with_snapshot,
                remove_disconnected_players,
                bullet_lifecycle,
            ),
        )
        .run();
}