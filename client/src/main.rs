mod components;
mod resources;
mod systems;

use std::collections::VecDeque;

use bevy::window::exit_on_primary_closed; // закрыть, когда окно закрыто
use bevy::{prelude::*, window::ExitCondition};
use bevy_quinnet::client::QuinnetClientPlugin;

use protocol::messages::Stance;
// Импортим общий протокол и адаптер
use protocol::constants::TICK_DT;

// Подмодули
use resources::*;
use systems::{
    bullet_lifecycle::bullet_lifecycle, disconnect::send_goodbye_and_close,
    exit::exit_if_goodbye_done, grab_my_id::grab_my_id, heartbeat::send_heartbeat,
    input::change_stance, interpolate_with_snapshot::interpolate_with_snapshot,
    network::receive_server_messages, rotate_to_cursor::rotate_to_cursor,
    send_input::send_input_and_predict, shoot::shoot_mouse, spawn_new_players::spawn_new_players,
    startup::setup,
};

use crate::systems::disconnect::GoodbyeSent;

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
        .insert_resource(GoodbyeSent::default())
        .insert_resource(HeartbeatTimer::default())
        // плагины
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CS‑style Multiplayer Client".into(),
                resolution: (800.0, 600.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(QuinnetClientPlugin::default())
        // системы
        .add_systems(Startup, setup)
        // .add_systems(
        //     Update,
        //     (
        //         grab_my_id,
        //         rotate_to_cursor,
        //         change_stance,
        //         shoot_mouse,
        //         send_input_and_predict,
        //         receive_server_messages,
        //         spawn_new_players,
        //         remove_disconnected_players,
        //         interpolate_with_snapshot,
        //         bullet_lifecycle,
        //     ),
        // )
        // 1) Сначала получаем новые снапшоты
        .add_systems(Update, receive_server_messages)
        // 2) Потом спавним появившихся игроков
        .add_systems(Update, spawn_new_players.after(receive_server_messages))
        // 3) Затем удаляем ушедших
        // .add_systems(
        //     Update,
        //     (
        //         send_goodbye_and_close,                             // кадр N
        //         exit_if_goodbye_done.after(send_goodbye_and_close), // кадр N+1
        //         exit_on_primary_closed, // чтобы «крестик» дал WindowCloseRequested
        //     ),
        // )
        // 4) Интерполируем всех
        .add_systems(Update, interpolate_with_snapshot)
        // 5) Лайф‑цикл пуль
        .add_systems(Update, bullet_lifecycle.after(interpolate_with_snapshot))
        // 6) Остальные системы ввода/рендера (по желанию тоже можете в нужном месте вставить)
        .add_systems(
            Update,
            (
                grab_my_id.before(receive_server_messages),
                rotate_to_cursor.before(shoot_mouse),
                change_stance.before(send_input_and_predict),
                shoot_mouse.before(send_input_and_predict),
                send_input_and_predict.before(receive_server_messages),
                send_heartbeat,
            ),
        )
        .run();
}
