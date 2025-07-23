use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use bevy_quinnet::client::certificate::CertificateVerificationMode;
use bevy_quinnet::client::connection::ClientEndpointConfiguration;
use bevy_quinnet::client::{QuinnetClient, QuinnetClientPlugin};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

mod game;
use game::{CommandEvent, SimpleGamePlugin};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlayerPosition {
    id: u64,
    x: f32,
    y: f32,
}

#[derive(Resource)]
struct Positions(HashMap<u64, Vec2>);

#[derive(Component)]
struct Interpolated {
    target: Vec2,
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

#[derive(Resource)]
struct MyPlayer {
    id: u64,
}

#[derive(Resource)]
struct SnapshotBuffer {
    snapshots: VecDeque<(f64, HashMap<u64, Vec2>)>, // time + positions
    delay: f64,                                     // seconds (например, 0.1 = 100 мс)
}

#[derive(Resource)]
struct SendTimer(Timer);

#[derive(Resource, Default)]
struct SpawnedPlayers(HashSet<u64>);

#[derive(Component)]
struct PlayerMarker(u64);

fn main() {
    App::new()
        .insert_resource(MyPlayer { id: rand::random() })
        .insert_resource(SnapshotBuffer {
            snapshots: VecDeque::new(),
            delay: 0.05, // 50ms lag for interpolation
        })
        .insert_resource(SendTimer(Timer::from_seconds(0.015, TimerMode::Repeating))) // 15 мс
        .insert_resource(SpawnedPlayers::default())
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
                // move_my_player,
                send_input,
                receive_snapshots,
                update_players,
                interpolate_with_snapshot,
            ),
        )
        .run();
}

fn setup(mut commands: Commands, my: Res<MyPlayer>, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d);

    // Подключаемся к серверу
    let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();
    let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    let cert_mode = CertificateVerificationMode::SkipVerification;

    let channels_config = ChannelsConfiguration::from_types(vec![ChannelKind::OrderedReliable {
        max_frame_size: 16_000,
    }])
    .unwrap();

    client
        .open_connection(endpoint_config, cert_mode, channels_config)
        .unwrap();

    // // Спавним локального игрока
    // commands.spawn((
    //     Sprite {
    //         color: Color::srgb(0.2, 1.0, 0.2),
    //         custom_size: Some(Vec2::splat(40.0)),
    //         ..default()
    //     },
    //     Transform::from_xyz(0.0, 0.0, 0.0),
    //     PlayerMarker(my.id),
    //     Interpolated { target: Vec2::ZERO },
    // ));
}

// fn move_my_player(
//     keys: Res<ButtonInput<KeyCode>>,
//     mut q: Query<&mut Transform, With<PlayerMarker>>,
//     time: Res<Time>,
// ) {
//     let speed = 200.0;
//     for mut t in q.iter_mut() {
//         let mut dir = Vec2::ZERO;
//         if keys.pressed(KeyCode::KeyW) {
//             dir.y += 1.0;
//         }
//         if keys.pressed(KeyCode::KeyS) {
//             dir.y -= 1.0;
//         }
//         if keys.pressed(KeyCode::KeyA) {
//             dir.x -= 1.0;
//         }
//         if keys.pressed(KeyCode::KeyD) {
//             dir.x += 1.0;
//         }
//         if dir.length() > 0.0 {
//             t.translation += (dir.normalize() * speed * time.delta_secs()).extend(0.0);
//         }
//     }
// }

fn send_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        let input = InputState {
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
        };
        let conn = client.connection_mut();
        let _ = conn.send_message_on(0, input);
    }
}

// fn send_position_with_timer(
//     time: Res<Time>,
//     mut timer: ResMut<SendTimer>,
//     mut client: ResMut<QuinnetClient>,
//     q: Query<(&Transform, &PlayerMarker)>,
// ) {
//     if timer.0.tick(time.delta()).just_finished() {
//         let conn = client.connection_mut();
//         for (t, marker) in q.iter() {
//             let pos = PlayerPosition {
//                 id: marker.0,
//                 x: t.translation.x,
//                 y: t.translation.y,
//             };
//             // "PositionUpdate" channel
//             let _ = conn.send_message_on(0, pos);
//         }
//     }
// }

// fn receive_positions(mut client: ResMut<QuinnetClient>, mut positions: ResMut<Positions>) {
//     let conn = client.connection_mut();
//     while let Some((_chan, snapshot)) = conn.try_receive_message::<Vec<PlayerPosition>>() {
//         positions.0.clear();
//         for p in snapshot {
//             positions.0.insert(p.id, Vec2::new(p.x, p.y));
//         }
//     }
// }

// fn receive_positions(mut endpoint: ResMut<QuinnetClient>, mut buffer: ResMut<SnapshotBuffer>) {
//     let conn = endpoint.connection_mut();

//     while let Some((_, msg)) = conn.try_receive_message::<Vec<PlayerPosition>>() {
//         let mut positions = HashMap::new();
//         for p in msg {
//             positions.insert(p.id, Vec2::new(p.x, p.y));
//         }
//         buffer.snapshots.push_back((time_in_seconds(), positions));

//         // Keep only last ~2 seconds of snapshots
//         while buffer.snapshots.len() > 128 {
//             buffer.snapshots.pop_front();
//         }
//     }
// }

fn receive_snapshots(mut endpoint: ResMut<QuinnetClient>, mut buffer: ResMut<SnapshotBuffer>) {
    let conn = endpoint.connection_mut();

    while let Some((_, msg)) = conn.try_receive_message::<Vec<PlayerSnapshot>>() {
        let mut positions = HashMap::new();
        for p in msg {
            positions.insert(p.id, Vec2::new(p.x, p.y));
        }
        buffer.snapshots.push_back((time_in_seconds(), positions));
        while buffer.snapshots.len() > 128 {
            buffer.snapshots.pop_front();
        }
    }
}

// fn update_other_players(
//     mut commands: Commands,
//     positions: Res<Positions>,
//     my: Res<MyPlayer>,
//     mut query: Query<(&PlayerMarker, &mut Interpolated)>,
// ) {
//     // Обновляем цели для интерполяции
//     for (marker, mut interp) in query.iter_mut() {
//         if let Some(&pos) = positions.0.get(&marker.0) {
//             interp.target = pos;
//         }
//     }

//     // Спавним новых игроков
//     for (&id, &pos) in positions.0.iter() {
//         if id != my.id && query.iter().all(|(m, _)| m.0 != id) {
//             commands.spawn((
//                 Sprite {
//                     color: Color::srgb(0.2, 0.4, 1.0),
//                     custom_size: Some(Vec2::splat(40.0)),
//                     ..default()
//                 },
//                 Transform::from_xyz(pos.x, pos.y, 0.0),
//                 PlayerMarker(id),
//                 Interpolated { target: pos },
//             ));
//         }
//     }
// }

// fn interpolate(mut q: Query<(&mut Transform, &Interpolated)>, time: Res<Time>) {
//     let alpha = 10.0 * time.delta_secs(); // Скорость интерполяции
//     for (mut t, interp) in q.iter_mut() {
//         let current = t.translation.truncate();
//         let new_pos = current.lerp(interp.target, alpha);
//         t.translation = new_pos.extend(0.0);
//     }
// }

// fn interpolate(
//     mut q: Query<(&mut Transform, &Interpolated, &PlayerMarker)>,
//     my: Res<MyPlayer>,
//     time: Res<Time>,
// ) {
//     let rate = 10. * time.delta_secs();
//     let epsilon = 0.5; // минимальный порог, чтобы не было дрожи

//     for (mut t, i, marker) in q.iter_mut() {
//         if marker.0 == my.id {
//             continue; // локального игрока не трогаем
//         }

//         let pos = t.translation.truncate();
//         let diff = i.target - pos;

//         // Если расстояние меньше epsilon — ничего не делаем
//         if diff.length_squared() < epsilon * epsilon {
//             continue;
//         }

//         let new = pos + diff * rate; // плавное движение
//         t.translation = new.extend(0.0);
//     }
// }

// fn update_players(
//     mut commands: Commands,
//     buffer: Res<SnapshotBuffer>,
//     my: Res<MyPlayer>,
//     mut spawned: ResMut<SpawnedPlayers>,
// ) {
//     if let Some((_, latest_positions)) = buffer.snapshots.back() {
//         for (&id, &pos) in latest_positions.iter() {
//             if id != my.id && !spawned.0.contains(&id) {
//                 commands.spawn((
//                     Sprite {
//                         color: Color::srgb(0.2, 0.4, 1.0), // синий
//                         custom_size: Some(Vec2::splat(40.0)),
//                         ..default()
//                     },
//                     Transform::from_xyz(pos.x, pos.y, 0.0),
//                     GlobalTransform::default(),
//                     PlayerMarker(id),
//                 ));
//                 spawned.0.insert(id);
//             }
//         }
//     }
// }

fn update_players(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    my: Res<MyPlayer>,
    mut spawned: ResMut<SpawnedPlayers>,
) {
    if let Some((_, latest)) = buffer.snapshots.back() {
        for (&id, &pos) in latest.iter() {
            if !spawned.0.contains(&id) {
                let color = if id == my.id {
                    Color::srgb(0.0, 1.0, 0.0)
                } else {
                    Color::srgb(0.2, 0.4, 1.0)
                };

                commands.spawn((
                    Sprite {
                        color,
                        custom_size: Some(Vec2::splat(40.0)),
                        ..default()
                    },
                    Transform::from_xyz(pos.x, pos.y, 0.0),
                    GlobalTransform::default(),
                    PlayerMarker(id),
                ));
                spawned.0.insert(id);
            }
        }
    }
}

// fn interpolate_with_snapshot(
//     mut q: Query<(&mut Transform, &PlayerMarker)>,
//     buffer: Res<SnapshotBuffer>,
//     my: Res<MyPlayer>,
// ) {
//     if buffer.snapshots.is_empty() {
//         return;
//     }

//     let render_time = time_in_seconds() - buffer.delay;

//     // Если только один снапшот → ставим как есть
//     if buffer.snapshots.len() == 1 {
//         let (_, positions) = &buffer.snapshots[0];
//         for (mut transform, marker) in q.iter_mut() {
//             if marker.0 != my.id {
//                 if let Some(pos) = positions.get(&marker.0) {
//                     transform.translation = pos.extend(0.0);
//                 }
//             }
//         }
//         return;
//     }

//     // Ищем два снапшота
//     let mut prev = None;
//     let mut next = None;

//     for i in 0..buffer.snapshots.len() {
//         if buffer.snapshots[i].0 <= render_time {
//             prev = Some(&buffer.snapshots[i]);
//         } else {
//             next = Some(&buffer.snapshots[i]);
//             break;
//         }
//     }

//     if let (Some((t0, pos0)), Some((t1, pos1))) = (prev, next) {
//         let alpha = ((render_time - t0) / (t1 - t0)).clamp(0.0, 1.0);
//         for (mut transform, marker) in q.iter_mut() {
//             if marker.0 != my.id {
//                 if let (Some(p0), Some(p1)) = (pos0.get(&marker.0), pos1.get(&marker.0)) {
//                     let interpolated = p0.lerp(*p1, alpha as f32);
//                     transform.translation = interpolated.extend(0.0);
//                 }
//             }
//         }
//     } else if let Some((_, positions)) = prev {
//         // Если только prev → показываем последнюю позицию
//         for (mut transform, marker) in q.iter_mut() {
//             if marker.0 != my.id {
//                 if let Some(pos) = positions.get(&marker.0) {
//                     transform.translation = pos.extend(0.0);
//                 }
//             }
//         }
//     }
// }

fn interpolate_with_snapshot(
    mut q: Query<(&mut Transform, &PlayerMarker)>,
    buffer: Res<SnapshotBuffer>,
) {
    if buffer.snapshots.len() < 2 {
        return;
    }

    let render_time = time_in_seconds() - buffer.delay;

    let mut prev = None;
    let mut next = None;

    for i in 0..buffer.snapshots.len() {
        if buffer.snapshots[i].0 <= render_time {
            prev = Some(&buffer.snapshots[i]);
        } else {
            next = Some(&buffer.snapshots[i]);
            break;
        }
    }

    if let (Some((t0, pos0)), Some((t1, pos1))) = (prev, next) {
        let alpha = ((render_time - t0) / (t1 - t0)).clamp(0.0, 1.0);
        for (mut transform, marker) in q.iter_mut() {
            if let (Some(p0), Some(p1)) = (pos0.get(&marker.0), pos1.get(&marker.0)) {
                let interpolated = p0.lerp(*p1, alpha as f32);
                transform.translation = interpolated.extend(0.0);
            }
        }
    }
}

fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    now.as_secs_f64()
}
