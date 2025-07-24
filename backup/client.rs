use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::certificate::CertificateVerificationMode;
use bevy_quinnet::client::connection::ClientEndpointConfiguration;
use bevy_quinnet::client::{QuinnetClient, QuinnetClientPlugin};
use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use serde::{Deserialize, Serialize};

// ===== Consts must match server =====
const TICK_DT: f32 = 0.015;
const MOVE_SPEED: f32 = 300.0;

const CH_INPUT: u8 = 0;
const CH_SHOOT: u8 = 1;
const CH_S2C: u8 = 2;

// ===== Components / Resources =====
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootFx {
    pub shooter_id: u64,
    pub from: Vec2,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Component)]
struct LocalPlayer;

#[derive(Component)]
struct PlayerMarker(u64);

#[derive(Component)]
struct Bullet {
    ttl: f32,
    vel: Vec2,
}

#[derive(Resource)]
struct MyPlayer {
    id: u64,
    got: bool,
}

#[derive(Resource)]
struct TimeSync {
    offset: f64, // local_now - server_time
}

#[derive(Resource)]
struct SnapshotBuffer {
    snapshots: VecDeque<WorldSnapshot>,
    delay: f64,
}

#[derive(Resource)]
struct CurrentStance(Stance);

#[derive(Resource)]
struct SendTimer(Timer);

#[derive(Resource, Default)]
struct SpawnedPlayers(HashSet<u64>);

#[derive(Resource)]
struct SeqCounter(u32);

#[derive(Resource, Default)]
struct PendingInputsClient(VecDeque<InputState>);

// ===== Messages =====
#[derive(Serialize, Deserialize, Clone, Debug)]
enum S2C {
    Snapshot(WorldSnapshot),
    ShootFx(ShootFx),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum Stance {
    Standing,
    Crouching,
    Prone,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct InputState {
    seq: u32,
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
    last_input_seq: HashMap<u64, u32>,
}

// ===== Main =====
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

// ===== Startup =====
fn setup(mut commands: Commands, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d::default());

    let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();
    let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    let cert_mode = CertificateVerificationMode::SkipVerification;

    let channels_config = ChannelsConfiguration::from_types(vec![
        ChannelKind::UnorderedReliable {
            max_frame_size: 16_000,
        }, // 0 InputState
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        }, // 1 ShootEvent
        ChannelKind::OrderedReliable {
            max_frame_size: 16_000,
        }, // 2 Snapshot+FX
    ])
    .unwrap();

    client
        .open_connection(endpoint_config, cert_mode, channels_config)
        .unwrap();
}

// ===== Systems =====

fn grab_my_id(client: Res<QuinnetClient>, mut me: ResMut<MyPlayer>, mut commands: Commands) {
    if me.got {
        return;
    }
    // Получаем идентификатор из Quinnet напрямую
    if let Some(id) = client.connection().client_id() {
        // client_id возвращает u64 напрямую
        me.id = id;
        me.got = true;
        // Спавним локального игрока, когда знаем id
        commands.spawn((
            Sprite {
                color: Color::srgb(0.0, 1.0, 0.0),
                custom_size: Some(Vec2::splat(40.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
            GlobalTransform::default(),
            PlayerMarker(me.id),
            LocalPlayer,
        ));
    }
}

fn rotate_to_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut player_q: Query<&mut Transform, With<LocalPlayer>>,
) {
    if let Ok(window) = windows.single() {
        if let Ok((camera, cam_tf)) = cam_q.single() {
            if let Ok(mut transform) = player_q.single_mut() {
                if let Some(cursor) = window.cursor_position() {
                    if let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor) {
                        let dir = world - transform.translation.truncate();
                        if dir.length_squared() > 0.0 {
                            transform.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
                        }
                    }
                }
            }
        }
    }
}

fn shoot_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&Transform, With<LocalPlayer>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    // Лог нажатия
    println!("🖱 [Client] Mouse Left pressed");

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => {
            println!("⚠️ [Client] Нет окна");
            return;
        }
    };
    let cursor = match window.cursor_position() {
        Some(c) => c,
        None => {
            println!("⚠️ [Client] Нет позиции курсора");
            return;
        }
    };
    let (camera, cam_tf) = match cam_q.single() {
        Ok(c) => c,
        Err(_) => {
            println!("⚠️ [Client] Нет камеры");
            return;
        }
    };
    let world = match camera.viewport_to_world_2d(cam_tf, cursor) {
        Ok(p) => p,
        Err(_) => {
            println!("⚠️ [Client] Не получилось в world");
            return;
        }
    };
    let player_pos = match player_q.single() {
        Ok(t) => t.translation.truncate(),
        Err(_) => {
            println!("⚠️ [Client] Нет LocalPlayer");
            return;
        }
    };
    let dir = (world - player_pos).normalize_or_zero();

    let shoot = ShootEvent {
        shooter_id: my.id,
        dir,
        timestamp: time_in_seconds(),
    };
    // Попытка отправки
    match client.connection_mut().send_message_on(CH_SHOOT, shoot.clone()) {
        Ok(_) => println!("📤 [Client] Отправили ShootEvent: {:?}", shoot),
        Err(e) => println!("❌ [Client] Ошибка отправки ShootEvent: {:?}", e),
    };

    // Локальный трассер
    println!("🎨 [Client] Локальный spawn_tracer");
    spawn_tracer(&mut commands, player_pos, dir);
}

fn change_stance(keys: Res<ButtonInput<KeyCode>>, mut stance: ResMut<CurrentStance>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Crouching,
            Stance::Crouching => Stance::Prone,
            Stance::Prone => Stance::Standing,
        };
    } else if keys.just_pressed(KeyCode::KeyE) {
        stance.0 = match stance.0 {
            Stance::Standing => Stance::Prone,
            Stance::Prone => Stance::Crouching,
            Stance::Crouching => Stance::Standing,
        };
    }
}

fn send_input_and_predict(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
    stance: Res<CurrentStance>,
    mut seq: ResMut<SeqCounter>,
    mut pending: ResMut<PendingInputsClient>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    if let Ok(mut t) = q.single_mut() {
        seq.0 = seq.0.wrapping_add(1);
        let inp = InputState {
            seq: seq.0,
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
            rotation: t.rotation.to_euler(EulerRot::XYZ).2,
            stance: stance.0.clone(),
            timestamp: time_in_seconds(),
        };
        client.connection_mut().send_message_on(CH_INPUT, inp.clone()).ok();
        pending.0.push_back(inp.clone());
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }
        simulate_input(&mut *t, &inp);
    }
}

fn receive_server_messages2(
    mut client: ResMut<QuinnetClient>,
    mut buffer: ResMut<SnapshotBuffer>,
    mut time_sync: ResMut<TimeSync>,
    my: Res<MyPlayer>,
    mut pending: ResMut<PendingInputsClient>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
    mut commands: Commands,
) {
    let conn = client.connection_mut();
    while let Some((_chan, msg)) = conn.try_receive_message::<S2C>() {
        match msg {
            S2C::Snapshot(snap) => {
                if buffer.snapshots.is_empty() {
                    time_sync.offset = time_in_seconds() - snap.server_time;
                }
                if let Ok(mut t) = q.single_mut() {
                    if let Some(ack) = snap.last_input_seq.get(&my.id) {
                        if let Some(ps) = snap.players.iter().find(|p| p.id == my.id) {
                            t.translation = Vec3::new(ps.x, ps.y, t.translation.z);
                            t.rotation = Quat::from_rotation_z(ps.rotation);
                            while let Some(front) = pending.0.front() {
                                if front.seq <= *ack {
                                    pending.0.pop_front();
                                } else {
                                    break;
                                }
                            }
                            for inp in pending.0.iter() {
                                simulate_input(&mut *t, inp);
                            }
                        }
                    }
                }
                buffer.snapshots.push_back(snap);
                while buffer.snapshots.len() > 120 {
                    buffer.snapshots.pop_front();
                }
            }
            S2C::ShootFx(fx) => {
                if fx.shooter_id != my.id {
                    spawn_tracer(&mut commands, fx.from, fx.dir);
                }
            }
        }
    }
}

fn receive_server_messages(
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
    mut buffer: ResMut<SnapshotBuffer>,
    mut time_sync: ResMut<TimeSync>,
    my: Res<MyPlayer>,
    mut pending: ResMut<PendingInputsClient>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
) {
    let conn = client.connection_mut();
    while let Some((chan, msg)) = conn.try_receive_message::<S2C>() {
        // теперь Snap+FX приходят на 2
        if chan != 2 {
            continue;
        }
        match msg {
            S2C::Snapshot(snap) => {
                // 1) Time sync
                if buffer.snapshots.is_empty() {
                    time_sync.offset = time_in_seconds() - snap.server_time;
                }
                // 2) Reconciliation for local player
                if let Ok(mut t) = q.single_mut() {
                    if let Some(ack) = snap.last_input_seq.get(&my.id) {
                        if let Some(ps) = snap.players.iter().find(|p| p.id == my.id) {
                            // Apply authoritative state
                            t.translation = Vec3::new(ps.x, ps.y, t.translation.z);
                            t.rotation = Quat::from_rotation_z(ps.rotation);
                            // Remove acknowledged inputs
                            while let Some(front) = pending.0.front() {
                                if front.seq <= *ack {
                                    pending.0.pop_front();
                                } else {
                                    break;
                                }
                            }
                            // Replay pending inputs
                            for inp in pending.0.iter() {
                                simulate_input(&mut *t, inp);
                            }
                        }
                    }
                }
                // 3) Buffer snapshots for interpolation
                buffer.snapshots.push_back(snap);
                while buffer.snapshots.len() > 120 {
                    buffer.snapshots.pop_front();
                }
            }
            S2C::ShootFx(fx) => {
                println!("💥 [Client] got FX from {} at {:?}", fx.shooter_id, fx.from);
                if fx.shooter_id != my.id {
                    commands.spawn((
                        Sprite {
                            color: Color::WHITE,
                            custom_size: Some(Vec2::new(12.0, 2.0)),
                            ..default()
                        },
                        Transform::from_translation(fx.from.extend(10.0))
                            .with_rotation(Quat::from_rotation_z(fx.dir.y.atan2(fx.dir.x))),
                        GlobalTransform::default(),
                        Bullet {
                            ttl: 0.35,
                            vel: fx.dir * 900.0,
                        },
                    ));
                }
            }
        }
    }
}

fn spawn_new_players(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    mut spawned: ResMut<SpawnedPlayers>,
    my: Res<MyPlayer>,
) {
    if let Some(last) = buffer.snapshots.back() {
        for p in &last.players {
            if p.id == my.id {
                continue;
            }
            if spawned.0.insert(p.id) {
                commands.spawn((
                    Sprite {
                        color: Color::srgb(0.2, 0.4, 1.0),
                        custom_size: Some(Vec2::splat(40.0)),
                        ..default()
                    },
                    Transform::from_xyz(p.x, p.y, 0.0)
                        .with_rotation(Quat::from_rotation_z(p.rotation)),
                    GlobalTransform::default(),
                    PlayerMarker(p.id),
                ));
            }
        }
    }
}

fn interpolate_with_snapshot(
    mut q: Query<(&mut Transform, &mut Sprite, &PlayerMarker)>,
    buffer: Res<SnapshotBuffer>,
    my: Res<MyPlayer>,
    time_sync: Res<TimeSync>,
) {
    if buffer.snapshots.len() < 2 {
        return;
    }
    let now_s = time_in_seconds() - time_sync.offset;
    let rt = now_s - buffer.delay;
    let (mut prev, mut next) = (None, None);
    for snap in buffer.snapshots.iter() {
        if snap.server_time <= rt {
            prev = Some(snap);
        } else {
            next = Some(snap);
            break;
        }
    }
    let (prev, next) = match (prev, next) {
        (Some(p), Some(n)) => (p, n),
        (Some(p), None) => (p, p),
        _ => return,
    };
    let t0 = prev.server_time;
    let t1 = next.server_time.max(t0 + 1e-4);
    let alpha = ((rt - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;
    let mut pmap = HashMap::new();
    for p in &prev.players {
        pmap.insert(p.id, p);
    }
    let mut nmap = HashMap::new();
    for p in &next.players {
        nmap.insert(p.id, p);
    }
    for (mut t, mut s, marker) in q.iter_mut() {
        if marker.0 == my.id {
            continue;
        }
        if let (Some(p0), Some(p1)) = (pmap.get(&marker.0), nmap.get(&marker.0)) {
            let from = Vec2::new(p0.x, p0.y);
            let to = Vec2::new(p1.x, p1.y);
            t.translation = from.lerp(to, alpha).extend(0.0);
            t.rotation = Quat::from_rotation_z(lerp_angle(p0.rotation, p1.rotation, alpha));
            s.color = stance_color(&p1.stance);
        }
    }
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

fn simulate_input(t: &mut Transform, inp: &InputState) {
    let mut dir = Vec2::ZERO;
    if inp.up {
        dir.y += 1.0;
    }
    if inp.down {
        dir.y -= 1.0;
    }
    if inp.left {
        dir.x -= 1.0;
    }
    if inp.right {
        dir.x += 1.0;
    }
    dir = dir.normalize_or_zero();
    t.translation += (dir * MOVE_SPEED * TICK_DT).extend(0.0);
    t.rotation = Quat::from_rotation_z(inp.rotation);
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
        GlobalTransform::default(), // ← add this
        Bullet {
            ttl: 0.35,
            vel: dir * 900.0,
        },
    ));
}

fn remove_disconnected_players(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    mut spawned: ResMut<SpawnedPlayers>,
    my: Res<MyPlayer>,
    q: Query<(Entity, &PlayerMarker)>,
) {
    // смотрим на последний снапшот
    if let Some(last) = buffer.snapshots.back() {
        // собираем текущие ID
        let current_ids: std::collections::HashSet<u64> =
            last.players.iter().map(|p| p.id).collect();

        for (entity, marker) in q.iter() {
            // не свой и уже не в списке
            if marker.0 != my.id && !current_ids.contains(&marker.0) {
                commands.entity(entity).despawn();
                spawned.0.remove(&marker.0);
                println!("🔌 Удалён игрок {}", marker.0);
            }
        }
    }
}

fn stance_color(s: &Stance) -> Color {
    match s {
        Stance::Standing => Color::srgb(0.20, 1.00, 0.20),
        Stance::Crouching => Color::srgb(0.15, 0.85, 1.00),
        Stance::Prone => Color::srgb(0.00, 0.60, 0.60),
    }
}

fn time_in_seconds() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % std::f32::consts::TAU;
    if diff.abs() > std::f32::consts::PI {
        diff -= diff.signum() * std::f32::consts::TAU;
    }
    a + diff * t
}
