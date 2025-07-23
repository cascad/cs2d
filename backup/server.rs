use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use bevy::prelude::*;
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use bevy_quinnet::server::{QuinnetServer, QuinnetServerPlugin, ServerEndpointConfiguration};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use bevy_quinnet::shared::ClientId;
use serde::{Deserialize, Serialize};

// ✅ Те же типы, что в клиенте
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CommandType {
    MoveTo(Vec2),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommandEvent {
    pub unit_id: u32,
    pub command: CommandType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlayerPosition {
    id: u64,
    x: f32,
    y: f32,
}

#[derive(Resource, Default)]
struct PlayerStates(HashMap<u64, (f32, f32)>);

fn main() {
    App::new()
        .insert_resource(PlayerStates::default())
        .add_plugins(DefaultPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_positions))
        .run();
}

fn setup(mut endpoint: ResMut<QuinnetServer>) {
    let server_ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let server_port = 6000;

    let endpoint_config = ServerEndpointConfiguration::from_ip(server_ip, server_port);

    let cert_mode = CertificateRetrievalMode::GenerateSelfSigned {
        server_hostname: "localhost".to_string(),
    };

    let channels_config = ChannelsConfiguration::from_types(vec![
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        },
        // .named("PositionUpdate"),
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        }, // .named("Snapshot"),
    ])
    .unwrap();

    endpoint
        .start_endpoint(endpoint_config, cert_mode, channels_config)
        .unwrap();

    println!("✅ Server started on {}:{}", server_ip, server_port);
}

fn handle_positions(mut server: ResMut<QuinnetServer>, mut state: ResMut<PlayerStates>) {
    let endpoint = server.endpoint_mut();

    // Получаем позиции от клиентов
    for client_id in endpoint.clients() {
        while let Some((_channel_id, msg)) =
            endpoint.try_receive_message_from::<PlayerPosition>(client_id)
        {
            println!("📥 Position from {}: {:?}", client_id, msg);
            state.0.insert(msg.id, (msg.x, msg.y));
        }
    }

    // Формируем snapshot
    let snapshot: Vec<PlayerPosition> = state
        .0
        .iter()
        .map(|(&id, &(x, y))| PlayerPosition { id, x, y })
        .collect();

    // Рассылаем команду всем клиентам
    if let Err(err) = endpoint.broadcast_message(snapshot.clone()) {
        eprintln!("❌ Broadcast failed: {:?}", err);
    } // else {
      //     println!("📤 Broadcasted command {:?}", snapshot);
      // }
}

// // ✅ Принимаем команды от клиентов и пересылаем всем
// fn receive_and_broadcast(mut server: ResMut<QuinnetServer>) {
//     let endpoint = server.endpoint_mut();

//     for client_id in endpoint.clients() {
//         while let Some((_channel_id, cmd)) =
//             endpoint.try_receive_message_from::<CommandEvent>(client_id)
//         {
//             println!("📥 Command from {}: {:?}", client_id, cmd);

//             // Рассылаем команду всем клиентам
//             if let Err(err) = endpoint.broadcast_message(cmd.clone()) {
//                 eprintln!("❌ Broadcast failed: {:?}", err);
//             } else {
//                 println!("📤 Broadcasted command {:?}", cmd);
//             }
//         }
//     }
// }

// // ✅ Просто тестовый broadcast каждые 2 сек
// fn broadcast_ping(mut server: ResMut<QuinnetServer>, time: Res<Time>) {
//     if time.elapsed_secs_f64() % 2.0 < 0.02 {
//         if let Err(err) = server
//             .endpoint_mut()
//             .broadcast_message("📣 Ping from server".to_string())
//         {
//             eprintln!("❌ Broadcast error: {:?}", err);
//         }
//     }
// }
