use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::certificate::CertificateVerificationMode;
use bevy_quinnet::client::connection::ClientEndpointConfiguration;
use bevy_quinnet::client::{QuinnetClient, QuinnetClientPlugin};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use serde::{Deserialize, Serialize};

#[derive(Resource)]
struct TimeSync {
    offset: f64,
} // local_now - server_time

#[derive(Component)]
struct Bullet {
    ttl: f32,
    vel: Vec2,
}

#[derive(Component)]
struct LocalPlayer;

// ---------- Сообщения (должны совпадать с сервером) ----------
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Stance {
    Standing,
    Crouching,
    Prone,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct InputState {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    rotation: f32,
    stance: Stance,
    timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ShootEvent {
    shooter_id: u64,
    dir: Vec2,
    timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlayerSnapshot {
    id: u64,
    x: f32,
    y: f32,
    rotation: f32,
    stance: Stance,
    hp: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct WorldSnapshot {
    players: Vec<PlayerSnapshot>,
    server_time: f64,
}

// ---------- Ресурсы ----------
#[derive(Resource)]
struct MyPlayer {
    id: u64,
    got: bool,
}

#[derive(Resource)]
struct SnapshotBuffer {
    snapshots: VecDeque<WorldSnapshot>, // буфер снапшотов с сервера
    delay: f64,                         // render delay (сек) для интерполяции
}

#[derive(Resource)]
struct SendTimer(Timer);

#[derive(Resource, Default)]
struct SpawnedPlayers(HashSet<u64>);

#[derive(Resource)]
struct CurrentStance(Stance);

// ---------- Компоненты ----------
#[derive(Component)]
struct PlayerMarker(u64);

// ---------- Main ----------
fn main() {
    App::new()
        .insert_resource(MyPlayer { id: 0, got: false })
        .insert_resource(SnapshotBuffer {
            snapshots: VecDeque::new(),
            delay: 0.05, // 50ms задержка для интерполяции
        })
        .insert_resource(SendTimer(Timer::from_seconds(0.015, TimerMode::Repeating))) // Отправка инпута ~64 Гц
        .insert_resource(SpawnedPlayers::default())
        .insert_resource(CurrentStance(Stance::Standing))
        .insert_resource(TimeSync { offset: 0.0 })
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
                rotate_to_cursor, // поворот локального игрока к мыши
                shoot_mouse,      // отправка ShootEvent
                change_stance,    // Q/E смена стойки
                send_input.run_if(on_timer(std::time::Duration::from_millis(15))),
                receive_snapshots,
                grab_my_id,
                spawn_new_players, // spawn_new_players должен идти до interpolate_with_snapshot, иначе интерполяция не найдёт сущность
                interpolate_with_snapshot,
                bullet_lifecycle,
                local_move,
            ),
        )
        .run();
}

// ---------- Startup ----------
fn setup(mut commands: Commands, my: Res<MyPlayer>, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d);

    // Подключаемся к серверу
    let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();
    let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    let cert_mode = CertificateVerificationMode::SkipVerification;

    // 0 - input/shoot, 1 - snapshots
    let channels_config = ChannelsConfiguration::from_types(vec![
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        },
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        },
    ])
    .unwrap();

    client
        .open_connection(endpoint_config, cert_mode, channels_config)
        .unwrap();

    // Спавним локального игрока заранее (цвет поменяем позже по стойке)
    commands.spawn((
        Sprite {
            color: Color::srgb(0.0, 1.0, 0.0),
            custom_size: Some(Vec2::splat(40.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        GlobalTransform::default(),
        PlayerMarker(my.id),
        LocalPlayer,
    ));
}

// ---------- Input / Shoot ----------
fn send_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
    stance: Res<CurrentStance>,
    q: Query<&Transform, With<LocalPlayer>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let Ok(t) = q.single() else {
        return;
    };

    let input = InputState {
        up: keys.pressed(KeyCode::KeyW),
        down: keys.pressed(KeyCode::KeyS),
        left: keys.pressed(KeyCode::KeyA),
        right: keys.pressed(KeyCode::KeyD),
        rotation: t.rotation.to_euler(EulerRot::XYZ).2,
        stance: stance.0.clone(),
        timestamp: time_in_seconds(),
    };

    let conn = client.connection_mut();
    let _ = conn.send_message_on(0, input);
}

fn rotate_to_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut player_q: Query<&mut Transform, With<LocalPlayer>>,
) {
    // Берём единственные окно / камеру / локального игрока
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam_q.single() else {
        return;
    };
    let Ok(mut transform) = player_q.single_mut() else {
        return;
    };

    // Позиция курсора в окне
    let Some(cursor_screen) = window.cursor_position() else {
        return;
    };

    // Переводим в мировые координаты
    let Ok(cursor_world) = camera.viewport_to_world_2d(cam_tf, cursor_screen) else {
        return;
    };

    // Направление от игрока к курсору
    let dir = cursor_world - transform.translation.truncate();
    if dir.length_squared() > 0.0 {
        let angle = dir.y.atan2(dir.x);
        transform.rotation = Quat::from_rotation_z(angle);
    }
}

fn shoot_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&Transform, (With<LocalPlayer>, Without<Camera>)>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    let (camera, cam_tf) = match cam_q.single() {
        Ok(c) => c,
        Err(_) => return,
    };
    let player_tf = match player_q.single() {
        Ok(t) => t,
        Err(_) => return,
    };

    let cursor = match window.cursor_position() {
        Some(c) => c,
        None => return,
    };
    let world_cursor = match camera.viewport_to_world_2d(cam_tf, cursor) {
        Ok(p) => p,
        Err(_) => return,
    };

    let player_pos = player_tf.translation.truncate();
    let dir = (world_cursor - player_pos).normalize_or_zero();

    // отправляем на сервер
    let shoot = ShootEvent {
        shooter_id: my.id,
        dir,
        timestamp: time_in_seconds(),
    };
    let conn = client.connection_mut();
    let _ = conn.send_message_on(0, shoot);

    // локальный трассер (если нужен)
    spawn_tracer(&mut commands, player_pos, dir);
}

fn change_stance(keys: Res<ButtonInput<KeyCode>>, mut stance: ResMut<CurrentStance>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Crouching,
            Stance::Crouching => Stance::Prone,
            Stance::Prone => Stance::Standing,
        };
    }
    if keys.just_pressed(KeyCode::KeyE) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Prone,
            Stance::Prone => Stance::Crouching,
            Stance::Crouching => Stance::Standing,
        };
    }
}

// ---------- Networking receive ----------
fn receive_snapshots(
    mut client: ResMut<QuinnetClient>,
    mut buffer: ResMut<SnapshotBuffer>,
    mut time_sync: ResMut<TimeSync>,
) {
    let conn = client.connection_mut();
    while let Some((chan, snap)) = conn.try_receive_message::<WorldSnapshot>() {
        if chan != 1 {
            continue;
        }

        if buffer.snapshots.is_empty() {
            // первый снапшот — зафиксируем offset
            time_sync.offset = time_in_seconds() - snap.server_time;
        }

        buffer.snapshots.push_back(snap);
        while buffer.snapshots.len() > 120 {
            buffer.snapshots.pop_front();
        }
    }
}

// ---------- Spawning / Updating ----------
fn spawn_new_players(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    mut spawned: ResMut<SpawnedPlayers>,
    my: Res<MyPlayer>,
) {
    let Some(last) = buffer.snapshots.back() else {
        return;
    };

    for p in &last.players {
        if p.id == my.id {
            continue;
        }
        if !spawned.0.contains(&p.id) {
            let color = Color::srgb(0.2, 0.4, 1.0);

            commands.spawn((
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(40.0)),
                    ..default()
                },
                Transform::from_xyz(p.x, p.y, 0.0).with_rotation(Quat::from_rotation_z(p.rotation)),
                GlobalTransform::default(),
                PlayerMarker(p.id),
            ));
            spawned.0.insert(p.id);
        }
    }
}

/// Интерполяция между снапшотами (позиция/ротейт/цвет по стойке)
fn interpolate_with_snapshot(
    mut q: Query<(&mut Transform, &mut Sprite, &PlayerMarker)>,
    buffer: Res<SnapshotBuffer>,
    my: Res<MyPlayer>,
    time_sync: Res<TimeSync>,
) {
    if buffer.snapshots.len() < 2 {
        return;
    }

    // переводим локальное время в "серверное"
    let now_server = time_in_seconds() - time_sync.offset;
    let render_time = now_server - buffer.delay;

    // ищем prev/next по server_time
    let (mut prev, mut next) = (None, None);
    for snap in buffer.snapshots.iter() {
        if snap.server_time <= render_time {
            prev = Some(snap)
        } else {
            next = Some(snap);
            break;
        }
    }
    let (prev, next) = match (prev, next) {
        (Some(p), Some(n)) => (p, n),
        // если нет next — берём последний (статично, без return)
        (Some(p), None) => (p, p),
        _ => return,
    };

    let t0 = prev.server_time;
    let t1 = next.server_time.max(t0 + 0.0001); // защита от деления на 0
    let alpha = ((render_time - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;

    use std::collections::HashMap;
    let mut pmap: HashMap<u64, &PlayerSnapshot> = HashMap::new();
    for p in &prev.players {
        pmap.insert(p.id, p);
    }
    let mut nmap: HashMap<u64, &PlayerSnapshot> = HashMap::new();
    for p in &next.players {
        nmap.insert(p.id, p);
    }

    for (mut transform, mut sprite, marker) in q.iter_mut() {
        // свой не трогаем (если делаешь prediction)
        if marker.0 == my.id {
            continue;
        }

        if let (Some(p0), Some(p1)) = (pmap.get(&marker.0), nmap.get(&marker.0)) {
            let from = Vec2::new(p0.x, p0.y);
            let to = Vec2::new(p1.x, p1.y);
            transform.translation = from.lerp(to, alpha).extend(0.0);

            let rot = lerp_angle(p0.rotation, p1.rotation, alpha);
            transform.rotation = Quat::from_rotation_z(rot);

            sprite.color = stance_color(&p1.stance);
        }
    }
}

// ---------- Helpers ----------
fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// Лерп углов (-pi..pi) по кратчайшему пути
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}

fn stance_color(s: &Stance) -> Color {
    match s {
        Stance::Standing => Color::srgb(0.20, 1.00, 0.20), // зелёный
        Stance::Crouching => Color::srgb(0.15, 0.85, 1.00), // циан/бирюзовый,
        Stance::Prone => Color::srgb(0.00, 0.60, 0.60),    // тёмный теал
    }
}

fn spawn_tracer(commands: &mut Commands, from: Vec2, dir: Vec2) {
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(12.0, 2.0)),
            ..default()
        },
        Transform::from_translation(from.extend(10.0))
            .with_rotation(Quat::from_rotation_z(dir.y.atan2(dir.x))),
        Bullet {
            ttl: 0.35,
            vel: dir * 900.0,
        },
    ));
}

fn bullet_lifecycle(
    mut q: Query<(Entity, &mut Transform, &mut Bullet)>,
    mut commands: Commands,
    time: Res<Time>,
) {
    for (e, mut t, mut b) in q.iter_mut() {
        b.ttl -= time.delta_secs();
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
        } else {
            t.translation += (b.vel * time.delta_secs()).extend(0.0);
        }
    }
}

fn local_move(
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
    time: Res<Time>,
) {
    let Ok(mut t) = q.single_mut() else {
        return;
    };
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir.x += 1.0;
    }
    if dir.length_squared() > 0.0 {
        t.translation += (dir.normalize() * 300.0 * time.delta_secs()).extend(0.0);
    }
}

fn grab_my_id(client: ResMut<QuinnetClient>, mut me: ResMut<MyPlayer>) {
    if me.got {
        return;
    }

    let conn = client.connection();
    if let Some(client_id) = conn.client_id() {
        // <-- этот id сервер видит
        me.id = client_id;
        me.got = true;
    };
}
