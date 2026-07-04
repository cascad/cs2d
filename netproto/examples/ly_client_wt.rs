//! Диагностика браузерного клиента БЕЗ браузера: тот же headless-клиент, что
//! ly_client, но транспорт — WebTransport (как в wasm-клиенте). Подключается к
//! серверу на порт 6001, digest сертификата читает из certificates/digest.txt.
//! Если репликация (NPC CONTRACT / predicted Position) идёт здесь, но не в
//! браузере — проблема в wasm-стороне; если не идёт и здесь — проблема в
//! серверном WebTransport-пути (MTU датаграмм и т.п.).
//! Запуск: `cargo run -p netproto --example ly_client_wt` (сервер должен работать).

use core::net::{Ipv4Addr, SocketAddr};
use core::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::ecs::lifecycle::Add;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use lightyear::netcode::NetcodeClient;
use lightyear::netcode::client_plugin::NetcodeConfig;
use lightyear::prelude::client::input::*;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::*;
use lightyear::prelude::*;
use netproto::messages::Fx;
use netproto::{
    AbilityState, AuthChannel, AuthDenied, AuthOk, Blocking, Health, Hello, MapGrids, NetInput,
    Npc, NpcRuntime, PRIVATE_KEY, PROTOCOL_ID, Player, Position, ProtocolPlugin, Rotation,
    Scoreboard, server_cfg, step_player, tick_duration,
};

/// Порт WebTransport-листенера сервера (см. server_config.toml webtransport_port).
const WT_PORT: u16 = 6001;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 120.0))),
    );
    app.add_plugins(LogPlugin {
        level: bevy::log::Level::INFO,
        // debug на aeronet_webtransport: видно дропы датаграмм больше MTU.
        // lightyear_replication/sync: видно, приходят ли реплики и идёт ли синк.
        filter: "aeronet_webtransport=debug,lightyear_replication=debug,lightyear_sync=debug".into(),
        ..Default::default()
    });
    app.add_plugins(ClientPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    app.add_systems(Startup, connect_client);
    app.add_systems(
        FixedPreUpdate,
        buffer_input.in_set(InputSystems::WriteClientInputs),
    );
    app.add_systems(FixedUpdate, player_movement);
    app.add_systems(
        Update,
        (send_hello, recv_auth, recv_scoreboard, log_positions, log_npc_contract, verdict_and_exit),
    );
    app.add_observer(recv_fx);
    app.add_observer(on_connected);
    app.add_observer(on_disconnected);
    app.add_observer(handle_predicted_spawn);
    app.run();
}

fn send_hello(
    mut sent: Local<bool>,
    mut sender: Query<&mut MessageSender<Hello>, With<Connected>>,
) {
    if *sent {
        return;
    }
    if let Ok(mut s) = sender.single_mut() {
        s.send::<AuthChannel>(Hello {
            name: "WtProbe".into(),
            password: "pw".into(),
        });
        *sent = true;
        info!("sent Hello (name=WtProbe)");
    }
}

fn recv_auth(
    mut ok: Query<&mut MessageReceiver<AuthOk>>,
    mut denied: Query<&mut MessageReceiver<AuthDenied>>,
) {
    for mut r in &mut ok {
        for _ in r.receive() {
            info!("AUTH OK from server");
        }
    }
    for mut r in &mut denied {
        for m in r.receive() {
            info!("AUTH DENIED: {}", m.reason);
        }
    }
}

fn recv_fx(trigger: On<RemoteEvent<Fx>>) {
    info!("FX: {:?}", trigger.event().trigger);
}

fn recv_scoreboard(mut sb: Query<&mut MessageReceiver<Scoreboard>>) {
    for mut r in &mut sb {
        for board in r.receive() {
            info!("SCOREBOARD: {} rows", board.0.len());
        }
    }
}

fn connect_client(mut commands: Commands) {
    let digest_raw = std::fs::read_to_string("certificates/digest.txt")
        .expect("certificates/digest.txt: запусти сервер, он создаст файл");
    // Как в client/src/lynet.rs: lightyear ждёт чистый hex без ':'.
    let digest: String = digest_raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();
    info!("cert digest = {digest}");

    let client_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0);
    let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), WT_PORT);
    let auth = Authentication::Manual {
        server_addr,
        client_id: 424242,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            ..default()
        },
    )
    .expect("netcode client");
    let entity = commands
        .spawn((
            Name::from("Client"),
            Client::default(),
            Link::new(None),
            LocalAddr(client_addr),
            PeerAddr(server_addr),
            ReplicationReceiver::default(),
            PredictionManager::default(),
            netcode,
            WebTransportClientIo {
                certificate_digest: digest,
            },
        ))
        .id();
    commands.trigger(Connect { entity });
    info!("client connecting to {server_addr} over WebTransport ...");
}

fn buffer_input(mut q: Query<&mut ActionState<NetInput>, With<InputMarker<NetInput>>>) {
    if let Ok(mut action) = q.single_mut() {
        action.0 = NetInput {
            aim: 0.0,
            ..default()
        };
    }
}

fn player_movement(
    mut q: Query<
        (
            &mut Position,
            &mut Rotation,
            &mut AbilityState,
            &mut Blocking,
            &ActionState<NetInput>,
        ),
        With<Predicted>,
    >,
    map: Option<Res<MapGrids>>,
) {
    let cfg = server_cfg();
    let walls = map.as_deref().map(|m| &m.movement);
    for (mut pos, mut rot, mut abil, mut blk, action) in &mut q {
        step_player(&mut pos, &mut rot, &mut abil, &mut blk, &action.0, &cfg, walls);
    }
}

/// Контракт клиентского моста NPC: если счётчики нулевые — репликация не доходит.
#[allow(clippy::type_complexity)]
fn log_npc_contract(
    time: Res<Time>,
    mut acc: Local<f32>,
    any_npc: Query<Entity, With<Npc>>,
    any_interp: Query<Entity, With<Interpolated>>,
    sync_ok: Query<
        Entity,
        (
            With<Npc>,
            With<Position>,
            With<Rotation>,
            With<Health>,
            With<NpcRuntime>,
            With<Interpolated>,
        ),
    >,
) {
    *acc += time.delta_secs();
    if *acc < 1.0 {
        return;
    }
    *acc = 0.0;
    info!(
        "NPC CONTRACT: any_npc={} any_interp={} sync_ok={}",
        any_npc.iter().count(),
        any_interp.iter().count(),
        sync_ok.iter().count(),
    );
}

fn on_connected(trigger: On<Add, Connected>) {
    info!("CONNECTED to server: {:?}", trigger.entity);
}

fn on_disconnected(trigger: On<Add, Disconnected>) {
    info!("DISCONNECTED: {:?}", trigger.entity);
}

fn handle_predicted_spawn(
    trigger: On<Add, Player>,
    predicted: Query<(), With<Predicted>>,
    mut commands: Commands,
) {
    if predicted.get(trigger.entity).is_ok() {
        commands
            .entity(trigger.entity)
            .insert(InputMarker::<NetInput>::default());
        info!("added InputMarker to predicted player {:?}", trigger.entity);
    }
}

fn log_positions(
    time: Res<Time>,
    mut acc: Local<f32>,
    me: Query<&Position, With<Predicted>>,
    npcs: Query<(&Position, &netproto::Health), (With<Npc>, With<Interpolated>)>,
) {
    *acc += time.delta_secs();
    if *acc < 1.0 {
        return;
    }
    *acc = 0.0;
    for pos in &me {
        info!("CLIENT predicted Position = ({:.1}, {:.1})", pos.0.x, pos.0.y);
    }
    info!("npcs visible = {}", npcs.iter().count());
}

/// Через 8 секунд выносим вердикт (дошла ли репликация) и выходим — чтобы можно
/// было гонять пробу в цикле и собирать статистику по интермиттентному багу.
fn verdict_and_exit(
    time: Res<Time>,
    players: Query<Entity, With<Player>>,
    predicted: Query<Entity, With<Predicted>>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs() < 8.0 {
        return;
    }
    let ok = !players.is_empty() && !predicted.is_empty();
    info!(
        "VERDICT: replication={} (players={} predicted={})",
        if ok { "OK" } else { "BROKEN" },
        players.iter().count(),
        predicted.iter().count(),
    );
    exit.write(AppExit::Success);
}
