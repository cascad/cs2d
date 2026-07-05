//! Сервер на Lightyear 0.26 (Bevy 0.18). Авторитет — ECS-сущности с
//! реплицируемыми компонентами протокола (`netproto`): движение/бой/способности в
//! `FixedUpdate` через общий детерминированный шаг, NPC/гранаты/туман войны/FX и
//! авторизация — системами `netproto`. Старый стек на `bevy_quinnet` (ручные
//! снапшоты + HashMap-состояние) заменён целиком; мини-лобби по TCP сохранено.

mod config;
mod net;
mod wt_cert;

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
use wt_cert::{write_digest_file, WtIdentity};
use netproto::{
    DamageAttribution, FxOut, MapGrids, NpcGrowls, NpcRespawns, PRIVATE_KEY,
    PROTOCOL_ID,     PendingStrikes, Player, ProtocolPlugin, ScoreboardDirty, Strikes,
    apply_player_inputs, flush_fx,
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
        // aeronet_webtransport=debug: видно дропы датаграмм («larger than MTU»)
        // с фактическим packet_len и mtu — диагностика браузерного клиента.
        filter: "server=info,netproto=info,lightyear_udp=off,aeronet_webtransport=debug".into(),
        ..Default::default()
    });
    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);

    // Лимит игроков (0 = без лимита) и персистентность аккаунтов: state/accounts.json
    // рядом с рабочим каталогом (в docker — том ./data/state, переживает рестарты).
    app.insert_resource(netproto::ServerLimits {
        max_players: cfg.max_players,
    });
    let accounts_path = std::path::PathBuf::from("state/accounts.json");
    app.insert_resource(netproto::load_accounts(&accounts_path));
    app.insert_resource(netproto::AccountsFile(accounts_path));

    app.insert_resource(cfg);
    app.insert_resource(meta);
    app.init_resource::<MapGrids>();
    app.init_resource::<Strikes>();
    app.init_resource::<PendingStrikes>();
    app.init_resource::<DamageAttribution>();
    app.init_resource::<ScoreboardDirty>();
    app.init_resource::<FxOut>();
    app.init_resource::<NpcRespawns>();
    app.init_resource::<NpcGrowls>();

    app.add_systems(Startup, (start_server, spawn_npcs));
    // Ввод игроков из InputBuffer → ActionState: обход бага lightyear с двумя
    // Server-сущностями (см. netproto::apply_player_inputs).
    app.add_systems(FixedPreUpdate, apply_player_inputs);
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

    // Диагностика (env CS2D_TICKLOG=1): каждый тик пишет позицию игроков с
    // номером тика — для сверки с предсказанием клиента (off-by-one и т.п.).
    if std::env::var("CS2D_TICKLOG").is_ok() {
        app.add_systems(
            FixedUpdate,
            debug_ticklog
                .after(server_simulate)
                .before(resolve_player_deaths),
        );
    }

    app.add_observer(on_new_client);
    app.add_observer(on_disconnect_cleanup);
    app.run();
}

/// Поднимаем netcode-сервер: UDP (нативный клиент) + WebTransport (браузер).
fn start_server(mut commands: Commands, cfg: Res<ServerConfig>) {
    let netcode_cfg = NetcodeConfig {
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        ..default()
    };

    // --- UDP ---
    let udp_addr = SocketAddr::new(cfg.ip_addr(), cfg.port);
    let udp_entity = commands
        .spawn((
            Name::from("ServerUdp"),
            NetcodeServer::new(netcode_cfg.clone()),
            LocalAddr(udp_addr),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity: udp_entity });
    info!("server listening on udp {udp_addr}");

    // --- WebTransport ---
    let wt = WtIdentity::from_config(&cfg);
    write_digest_file(&wt.digest);
    let wt_addr = SocketAddr::new(cfg.ip_addr(), cfg.webtransport_port);
    let wt_entity = commands
        .spawn((
            Name::from("ServerWebTransport"),
            NetcodeServer::new(netcode_cfg),
            LocalAddr(wt_addr),
            WebTransportServerIo {
                certificate: wt.identity,
            },
        ))
        .id();
    commands.trigger(Start { entity: wt_entity });
    info!("server listening on webtransport {wt_addr}");
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

/// Диагностика CS2D_TICKLOG: тик и позиция каждого игрока после server_simulate.
fn debug_ticklog(
    timeline: Res<LocalTimeline>,
    players: Query<&netproto::Position, With<Player>>,
) {
    for pos in &players {
        info!("TICKLOG srv tick={:?} x={:.3} y={:.3}", timeline.tick(), pos.0.x, pos.0.y);
    }
}
