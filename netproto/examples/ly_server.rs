//! Stage 4: сервер-авторитет на Lightyear. На коннекте спавнит игрока
//! (Replicate + Prediction/Interpolation targets), в FixedUpdate двигает игроков
//! по полученному вводу через общий `step_player`.
//! Запуск: `cargo run -p netproto --example ly_server`.

use core::net::{Ipv4Addr, SocketAddr};
use core::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::ecs::lifecycle::Add;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use netproto::{
    AbilityState, Accounts, Blocking, DamageAttribution, FxOut, Health, PRIVATE_KEY, PROTOCOL_ID, Position,
    ProtocolPlugin, Rotation, SERVER_PORT, ScoreboardDirty, Strikes, flush_fx,
    broadcast_scoreboard, npc_ai, on_disconnect_cleanup, resolve_player_deaths, server_handle_auth,
    server_simulate, spawn_grenades, spawn_npc, tick_duration, update_grenades, update_interest,
};
use protocol::abilities::Abilities;
use protocol::messages::NpcKind;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())),
    );
    app.add_plugins(LogPlugin::default());
    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    app.init_resource::<Strikes>();
    app.init_resource::<netproto::PendingStrikes>();
    app.init_resource::<DamageAttribution>();
    app.init_resource::<Accounts>();
    app.init_resource::<ScoreboardDirty>();
    app.init_resource::<FxOut>();
    app.init_resource::<netproto::NpcRespawns>();
    app.add_systems(Startup, (start_server, spawn_dummy, spawn_npcs));
    // Авторитетный тик по порядку: движение+резолв ударов → ИИ неписей →
    // спавн брошенных гранат → физика/детонация гранат.
    app.add_systems(
        FixedUpdate,
        (
            server_simulate,
            npc_ai,
            spawn_grenades,
            update_grenades,
            resolve_player_deaths,
            netproto::respawn_players,
        )
            .chain(),
    );
    app.add_systems(
        Update,
        (server_handle_auth, broadcast_scoreboard, flush_fx),
    );
    // Туман войны: обновляем зоны интереса до буферизации репликации.
    app.add_systems(
        PostUpdate,
        update_interest.before(ReplicationBufferSystems::Buffer),
    );
    app.add_systems(Update, log_server_players);
    app.add_observer(on_new_client);
    app.add_observer(on_connected);
    app.add_observer(on_disconnected);
    app.add_observer(on_disconnect_cleanup);
    app.run();
}

/// ДИАГНОСТИКА: позиция авторитетного игрока + применённый ввод (вправо?), чтобы
/// сверить с предсказанием клиента (убегает ли клиент от сервера).
fn log_server_players(
    time: Res<Time>,
    mut acc: Local<f32>,
    q: Query<
        (
            &Position,
            Option<&lightyear::prelude::input::native::ActionState<netproto::NetInput>>,
        ),
        With<netproto::Player>,
    >,
) {
    *acc += time.delta_secs();
    if *acc < 0.25 {
        return;
    }
    *acc = 0.0;
    for (pos, action) in &q {
        let right = action.map(|a| a.0.right).unwrap_or(false);
        let has = action.is_some();
        info!(
            "SERVER player Position = ({:.1}, {:.1})  input.right={right} has_action={has}",
            pos.0.x, pos.0.y
        );
    }
}

/// Stage 6: непись для проверки серверного ИИ + репликации. Одна агрит и идёт на
/// игрока (он её бьёт → смерть → despawn), вторая далеко — остаётся idle у «дома».
fn spawn_npcs(mut commands: Commands) {
    // chaser в зоне интереса (видна), far — за VIEW_RADIUS(1400) (туман войны культит).
    let a = spawn_npc(&mut commands, NpcKind::Skeleton, Vec2::new(200.0, 0.0));
    let b = spawn_npc(&mut commands, NpcKind::Zombie, Vec2::new(0.0, 2000.0));
    info!("spawned npcs: chaser {a:?} @ (200,0), far {b:?} @ (0,2000)");
}

/// Stage 5: неподвижная «груша» для проверки серверного резолва ближнего боя.
/// Стоит справа от точки спавна; подошедший клиент бьёт по ней — сервер логирует
/// попадания/станы. Высокий HP, чтобы пережить серию ударов.
fn spawn_dummy(mut commands: Commands) {
    let dummy = commands
        .spawn((
            Name::from("Dummy"),
            Position(Vec2::new(60.0, 0.0)), // в пределах MELEE_RANGE(56)+radius(16)
            Rotation(core::f32::consts::PI), // смотрит влево (на игрока) — на блок не влияет
            Health(500),
            AbilityState(Abilities::default()),
            Blocking(false),
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
        ))
        .id();
    info!("spawned dummy target {dummy:?} at (60,0)");
}

fn start_server(mut commands: Commands) {
    let server_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), SERVER_PORT);
    let entity = commands
        .spawn((
            Name::from("Server"),
            NetcodeServer::new(NetcodeConfig {
                protocol_id: PROTOCOL_ID,
                private_key: PRIVATE_KEY,
                ..default()
            }),
            LocalAddr(server_addr),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity });
    info!("server listening on udp :{SERVER_PORT}");
}

fn on_new_client(trigger: On<Add, LinkOf>, mut commands: Commands) {
    commands.entity(trigger.entity).insert((
        ReplicationSender::new(Duration::ZERO, SendUpdatesMode::SinceLastAck, false),
        Name::from("ClientOf"),
    ));
    info!("link established: {:?}", trigger.entity);
}

/// Клиент прошёл рукопожатие. Игрока НЕ спавним здесь — ждём `Hello` и успешную
/// авторизацию (`server_handle_auth`), только тогда появляется сущность игрока.
fn on_connected(trigger: On<Add, Connected>, query: Query<&RemoteId>) {
    if let Ok(remote) = query.get(trigger.entity) {
        info!("client CONNECTED (awaiting auth): link={:?} peer={:?}", trigger.entity, remote.0);
    }
}

fn on_disconnected(trigger: On<Add, Disconnected>) {
    info!("client DISCONNECTED: {:?}", trigger.entity);
}
