//! Сервер на Lightyear 0.26 (Bevy 0.18). Авторитет — ECS-сущности с
//! реплицируемыми компонентами протокола (`netproto`): движение/бой/способности в
//! `FixedUpdate` через общий детерминированный шаг, NPC/гранаты/туман войны/FX и
//! авторизация — системами `netproto`. Старый стек на `bevy_quinnet` (ручные
//! снапшоты + HashMap-состояние) заменён целиком; мини-лобби по TCP сохранено.

mod config;
mod net;

use core::net::SocketAddr;
use core::time::Duration;
use std::sync::atomic::Ordering;

use bevy::app::{ScheduleRunnerPlugin, ctrlc};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use config::ServerConfig;
use net::{MetaPlayerCount, start_meta_endpoint};
use netproto::{
    Accounts, DamageAttribution, FxOut, MapGrids, NpcGrowls, NpcRespawns, PRIVATE_KEY,
    PROTOCOL_ID,     PendingStrikes, Player, ProtocolPlugin, ScoreboardDirty, Strikes, flush_fx,
    broadcast_scoreboard, npc_ai, npc_growls, on_disconnect_cleanup, resolve_player_deaths,
    respawn_npcs, respawn_players, server_handle_auth, server_simulate, spawn_grenades, spawn_npc,
    tick_duration, update_grenades, update_interest,
};
use protocol::maps;
use protocol::messages::NpcKind;

fn main() {
    ctrlc::set_handler(|| {
        println!("⚡ Server shutting down");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let cfg = config::load_or_create();
    // Мини-лобби (TCP) на том же порту: отдаёт имя/онлайн/лимит. Счётчик игроков
    // обновляем из числа сущностей-игроков каждый кадр.
    let meta = start_meta_endpoint(cfg.ip_addr(), cfg.port, cfg.name.clone(), cfg.max_players);

    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())),
    );
    app.add_plugins(LogPlugin {
        level: bevy::log::Level::INFO,
        // lightyear_udp=off: на Windows после дисконнекта клиента recv ловит
        // WSAECONNRESET (10054) на КАЖДЫЙ пакет до таймаута netcode — сотни
        // строк ERROR-спама в секунду без пользы (соединение и так закрывается).
        filter: "server=info,netproto=info,lightyear_udp=off".into(),
        ..Default::default()
    });
    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);

    app.insert_resource(cfg);
    app.insert_resource(meta);
    app.init_resource::<MapGrids>();
    app.init_resource::<Strikes>();
    app.init_resource::<PendingStrikes>();
    app.init_resource::<DamageAttribution>();
    app.init_resource::<Accounts>();
    app.init_resource::<ScoreboardDirty>();
    app.init_resource::<FxOut>();
    app.init_resource::<NpcRespawns>();
    app.init_resource::<NpcGrowls>();

    app.add_systems(Startup, (start_server, spawn_npcs));
    // Авторитетный тик: движение+бой → ИИ неписей → спавн гранат → физика гранат →
    // единая точка смертей (PvP/NPC/гранаты).
    app.add_systems(
        FixedUpdate,
        (
            server_simulate,
            npc_ai,
            respawn_npcs,
            npc_growls,
            spawn_grenades,
            update_grenades,
            resolve_player_deaths,
            respawn_players,
        )
            .chain(),
    );
    // Сообщения: авторизация, таблица очков, FX; плюс счётчик игроков для лобби.
    app.add_systems(
        Update,
        (
            server_handle_auth,
            broadcast_scoreboard,
            flush_fx,
            update_meta_player_count,
        ),
    );
    // Туман войны: обновляем зоны интереса до буферизации репликации.
    app.add_systems(
        PostUpdate,
        update_interest.before(ReplicationBufferSystems::Buffer),
    );

    app.add_observer(on_new_client);
    app.add_observer(on_disconnect_cleanup);
    app.run();
}

/// Поднимаем netcode-сервер на UDP-порту из конфига.
fn start_server(mut commands: Commands, cfg: Res<ServerConfig>) {
    let server_addr = SocketAddr::new(cfg.ip_addr(), cfg.port);
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
    info!("server listening on udp {server_addr}");
}

/// Спавним неписей из точек спавна общей карты (чередуем скелет/зомби).
fn spawn_npcs(mut commands: Commands) {
    let spawns = maps::npc_spawns(netproto::TILE);
    for (i, pos) in spawns.into_iter().enumerate() {
        let kind = if i % 2 == 0 {
            NpcKind::Skeleton
        } else {
            NpcKind::Zombie
        };
        spawn_npc(&mut commands, kind, pos);
    }
}

/// Новое соединение (`ClientOf`): вешаем отправитель репликации. Спавн игрока —
/// после авторизации (`server_handle_auth`).
fn on_new_client(trigger: On<Add, LinkOf>, mut commands: Commands) {
    commands.entity(trigger.entity).insert((
        // Шлём апдейты КАЖДЫЙ тик (Duration::ZERO): частые мелкие подтверждения
        // → маленькие окна отката у клиента → меньше «дёрганья» предсказания.
        ReplicationSender::new(Duration::ZERO, SendUpdatesMode::SinceLastAck, false),
        Name::from("ClientOf"),
    ));
    info!("link established: {:?}", trigger.entity);
}

/// Обновляем счётчик игроков для меты лобби (число сущностей-игроков).
fn update_meta_player_count(meta: Res<MetaPlayerCount>, players: Query<(), With<Player>>) {
    meta.0.store(players.iter().count() as u32, Ordering::Relaxed);
}
