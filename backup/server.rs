use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use bevy::prelude::*;
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use bevy_quinnet::server::{QuinnetServer, QuinnetServerPlugin, ServerEndpointConfiguration};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
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

#[derive(Serialize, Deserialize, Clone)]
struct InputState {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerSnapshot {
    id: u64,
    x: f32,
    y: f32,
}

#[derive(Resource, Default)]
struct PlayerStates(HashMap<u64, Vec2>);

#[derive(Resource)]
struct ServerTickTimer(Timer);

fn main() {
    App::new()
        .insert_resource(PlayerStates::default())
        .insert_resource(ServerTickTimer(Timer::from_seconds(
            0.015,
            TimerMode::Repeating,
        ))) // 64Hz
        .add_plugins(MinimalPlugins)
        .add_plugins(QuinnetServerPlugin::default())
        .add_systems(Startup, start_server)
        .add_systems(Update, (process_inputs, server_tick))
        .run();
}

fn start_server(mut endpoint: ResMut<QuinnetServer>) {
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

fn process_inputs(mut server: ResMut<QuinnetServer>, mut states: ResMut<PlayerStates>) {
    let endpoint = server.endpoint_mut();

    // Получаем позиции от клиентов
    for client_id in endpoint.clients() {
        while let Some((_channel_id, input)) =
            endpoint.try_receive_message_from::<InputState>(client_id)
        {
            // println!("📥 Position from {}: {:?}", client_id, input);
            let speed = 300.0;
            let mut dir = Vec2::ZERO;
            if input.up {
                dir.y += 1.0;
            }
            if input.down {
                dir.y -= 1.0;
            }
            if input.left {
                dir.x -= 1.0;
            }
            if input.right {
                dir.x += 1.0;
            }
            dir = dir.normalize_or_zero();
            let pos = states.0.entry(client_id).or_insert(Vec2::ZERO);
            *pos += dir * speed * 0.015; // Fixed tick time
        }
    }
}

fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    states: Res<PlayerStates>,
    mut server: ResMut<QuinnetServer>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        let snapshot: Vec<PlayerSnapshot> = states
            .0
            .iter()
            .map(|(&id, &pos)| PlayerSnapshot {
                id,
                x: pos.x,
                y: pos.y,
            })
            .collect();

        let endpoint = server.endpoint_mut();

        if let Err(err) = endpoint.broadcast_message(snapshot.clone()) {
            eprintln!("❌ Broadcast failed: {:?}", err);
        } // else {
          //     println!("📤 Broadcasted command {:?}", snapshot);
          // }
    }
}
