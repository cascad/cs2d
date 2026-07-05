//! Диагностический бот для замера рубербендинга: подключается по UDP (как
//! нативный клиент), зеркалит сетевой конфиг реального клиента (input delay /
//! interpolation), жмёт «вправо» и меряет качество предсказания:
//!   - предсказанная Position каждые 0.5с;
//!   - число откатов (PredictionMetrics.rollbacks) и ре-симулированных тиков;
//!   - «рывки назад» (кадры, где x уменьшился при зажатом «вправо»);
//!   - итоговый VERDICT.
//! Запуск: сервер должен работать; `cargo run -p netproto --example ly_bot_move`.
//! Переменные: BOT_ADDR=127.0.0.1:6000, BOT_SECS=12.

use core::net::SocketAddr;
use core::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::ecs::lifecycle::Add;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use lightyear::netcode::NetcodeClient;
use lightyear::netcode::client_plugin::NetcodeConfig;
use lightyear::prediction::diagnostics::PredictionMetrics;
use lightyear::interpolation::timeline::InterpolationConfig;
use lightyear::prelude::client::{InputDelayConfig, InputTimelineConfig};
use lightyear::prelude::client::input::*;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::*;
use lightyear::prelude::*;
use netproto::{
    AbilityState, AuthChannel, AuthDenied, AuthOk, Blocking, Hello, MapGrids, NetInput,
    PRIVATE_KEY, PROTOCOL_ID, Player, Position, ProtocolPlugin, Rotation, server_cfg,
    step_player, tick_duration,
};

#[derive(Resource, Default)]
struct Stats {
    prev_x: Option<f32>,
    yanks: u32,
    max_yank: f32,
    moving_since: Option<f64>,
}

fn main() {
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 120.0))),
    );
    app.add_plugins(LogPlugin {
        level: bevy::log::Level::INFO,
        filter: "lightyear_sync=warn".into(),
        ..Default::default()
    });
    app.add_plugins(ClientPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    app.init_resource::<Stats>();
    // Карта коллизий — КАК В РЕАЛЬНОМ КЛИЕНТЕ: без неё предсказание проходит
    // сквозь стены и расходится с сервером у каждой стены.
    app.init_resource::<MapGrids>();
    app.add_systems(Startup, connect_client);
    app.add_systems(
        FixedPreUpdate,
        buffer_input.in_set(InputSystems::WriteClientInputs),
    );
    app.add_systems(FixedUpdate, (player_movement, debug_ticklog).chain());
    app.add_systems(Update, (send_hello, recv_auth, track_stats, report, verdict_and_exit));
    app.add_observer(handle_predicted_spawn);
    app.add_observer(on_connected);
    app.run();
}

fn connect_client(mut commands: Commands) {
    let addr = std::env::var("BOT_ADDR").unwrap_or_else(|_| "127.0.0.1:6000".into());
    let server_addr: SocketAddr = addr.parse().expect("BOT_ADDR ip:port");
    let client_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let client_id = 77_000
        + std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64 % 997)
            .unwrap_or(1);
    let auth = Authentication::Manual {
        server_addr,
        client_id,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            // как в клиенте: токен живёт час, иначе сдвиг часов бот↔сервер
            // > 30с даёт ложный «connection request timed out»
            token_expire_secs: 3600,
            ..default()
        },
    )
    .expect("netcode client");
    // ЗЕРКАЛО реального клиента (client/src/lynet.rs connect_to): те же тайминги.
    // BOT_DELAY=none — без искусственной задержки ввода (диагностика off-by-one).
    let input_timeline = if std::env::var("BOT_DELAY").as_deref() == Ok("none") {
        InputTimelineConfig::default()
    } else {
        InputTimelineConfig::default().with_input_delay(InputDelayConfig::balanced())
    };
    let interpolation = InterpolationConfig::default()
        .with_min_delay(Duration::from_millis(10))
        .with_send_interval_ratio(1.7);
    let base = (
        Name::from("MoveBot"),
        Client::default(),
        Link::new(None),
        LocalAddr(client_addr),
        PeerAddr(server_addr),
        ReplicationReceiver::default(),
        PredictionManager::default(),
        input_timeline,
        interpolation,
        netcode,
    );
    // BOT_WT=1 — WebTransport (транспорт браузерного клиента), digest из
    // certificates/digest.txt; иначе UDP (как нативный клиент).
    let wt = std::env::var("BOT_WT").is_ok();
    let entity = if wt {
        let digest_raw = std::fs::read_to_string("certificates/digest.txt")
            .expect("certificates/digest.txt: запусти сервер");
        let digest: String = digest_raw
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>()
            .to_lowercase();
        commands
            .spawn((base, WebTransportClientIo { certificate_digest: digest }))
            .id()
    } else {
        commands.spawn((base, UdpIo::default())).id()
    };
    commands.trigger(Connect { entity });
    info!("bot connecting to {server_addr} (wt={wt}) ...");
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
            name: "MoveBot".into(),
            password: "pw".into(),
        });
        *sent = true;
        info!("sent Hello (MoveBot)");
    }
}

fn recv_auth(
    mut ok: Query<&mut MessageReceiver<AuthOk>>,
    mut denied: Query<&mut MessageReceiver<AuthDenied>>,
) {
    for mut r in &mut ok {
        for _ in r.receive() {
            info!("AUTH OK");
        }
    }
    for mut r in &mut denied {
        for m in r.receive() {
            error!("AUTH DENIED: {}", m.reason);
        }
    }
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
        info!("predicted player spawned, input attached");
    }
}

fn on_connected(trigger: On<Add, Connected>) {
    info!("CONNECTED: {:?}", trigger.entity);
}

/// Режимы: BOT_MODE=move (влево по коридору, прицел фиксирован) |
/// aim (стоим, прицел вращается) | move_aim (и то и другое, по умолчанию) |
/// patrol (туда-сюда по 8с — сущности выходят из зоны интереса и возвращаются:
/// проверка повторной репликации после тумана войны).
fn buffer_input(
    time: Res<Time>,
    mut q: Query<&mut ActionState<NetInput>, With<InputMarker<NetInput>>>,
) {
    let mode = std::env::var("BOT_MODE").unwrap_or_else(|_| "move_aim".into());
    if let Ok(mut action) = q.single_mut() {
        let t = time.elapsed_secs();
        let aim = if mode == "move" || mode == "patrol" {
            0.0
        } else {
            (t * 0.8).sin() * 1.2
        };
        let (mut left, mut right) = (mode != "aim", false);
        if mode == "patrol" {
            let phase_right = (t / 8.0) as u32 % 2 == 1;
            left = !phase_right;
            right = phase_right;
        }
        action.0 = NetInput {
            left,
            right,
            aim,
            ..default()
        };
    }
}

/// То же предсказание, что в реальном клиенте.
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

/// Каждый кадр: ловим «рывки назад» по предсказанной позиции.
fn track_stats(
    time: Res<Time>,
    mut stats: ResMut<Stats>,
    me: Query<&Position, With<Predicted>>,
) {
    let Ok(pos) = me.single() else { return };
    if stats.moving_since.is_none() {
        stats.moving_since = Some(time.elapsed_secs_f64());
    }
    if let Some(prev) = stats.prev_x {
        let dx = pos.0.x - prev;
        // при зажатом «влево» x должен монотонно убывать; заметный шаг назад = рывок
        if dx > 1.0 {
            stats.yanks += 1;
            stats.max_yank = stats.max_yank.max(dx);
        }
    }
    stats.prev_x = Some(pos.0.x);
}

#[allow(clippy::too_many_arguments)]
fn report(
    time: Res<Time>,
    mut acc: Local<f32>,
    me: Query<&Position, With<Predicted>>,
    confirmed: Query<&Confirmed<Position>>,
    metrics: Option<Res<PredictionMetrics>>,
    stats: Res<Stats>,
    // Контракт видимости: сущности, пере-реплицированные после выхода/входа в
    // зону интереса (туман войны), ОБЯЗАНЫ снова получать Interpolated. Если
    // npc_conf > npc_interp устойчиво — флаги записи отправителя потерялись.
    npc_conf: Query<(), With<netproto::Npc>>,
    npc_interp: Query<(), (With<netproto::Npc>, With<Interpolated>)>,
    plr_interp: Query<(), (With<netproto::Player>, With<Interpolated>)>,
) {
    *acc += time.delta_secs();
    if *acc < 0.5 {
        return;
    }
    *acc = 0.0;
    let p = me.single().map(|p| p.0).unwrap_or(Vec2::NAN.into());
    let c = confirmed
        .iter()
        .next()
        .map(|c| c.0.0)
        .unwrap_or(Vec2::NAN.into());
    let (rb, rbt) = metrics.map(|m| (m.rollbacks, m.rollback_ticks)).unwrap_or((0, 0));
    info!(
        "STATS: pred=({:.1},{:.1}) conf=({:.1},{:.1}) gap={:.1} rollbacks={} rb_ticks={} yanks={} max_yank={:.1} npc={}/{}interp plr_interp={}",
        p.x, p.y, c.x, c.y, p.distance(c), rb, rbt, stats.yanks, stats.max_yank,
        npc_conf.iter().count(), npc_interp.iter().count(), plr_interp.iter().count()
    );
}

/// Диагностика BOT_TICKLOG: тик + предсказанная позиция (после player_movement)
/// и подтверждённая позиция с тиком её пакета — для сверки с сервером.
fn debug_ticklog(
    timeline: Res<lightyear::prelude::LocalTimeline>,
    me: Query<&Position, With<Predicted>>,
    confirmed: Query<(&Confirmed<Position>, &lightyear::prelude::ConfirmedTick)>,
) {
    if std::env::var("BOT_TICKLOG").is_err() {
        return;
    }
    if let Ok(pos) = me.single() {
        info!("TICKLOG cli tick={:?} x={:.3} y={:.3}", timeline.tick(), pos.0.x, pos.0.y);
    }
    if let Some((c, ct)) = confirmed.iter().next() {
        info!("TICKLOG conf tick={:?} x={:.3} y={:.3}", ct.tick, c.0.0.x, c.0.0.y);
    }
}

fn verdict_and_exit(
    time: Res<Time>,
    stats: Res<Stats>,
    metrics: Option<Res<PredictionMetrics>>,
    me: Query<&Position, With<Predicted>>,
    mut exit: MessageWriter<AppExit>,
) {
    let secs: f64 = std::env::var("BOT_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(12.0);
    if time.elapsed_secs_f64() < secs {
        return;
    }
    let moved = stats
        .moving_since
        .map(|t0| time.elapsed_secs_f64() - t0)
        .unwrap_or(0.0);
    let x = me.single().map(|p| p.0.x).unwrap_or(f32::NAN);
    let (rb, rbt) = metrics.map(|m| (m.rollbacks, m.rollback_ticks)).unwrap_or((0, 0));
    info!(
        "VERDICT: moved_secs={moved:.1} final_x={x:.1} rollbacks={rb} rb_ticks={rbt} yanks={} max_yank={:.1}",
        stats.yanks, stats.max_yank
    );
    exit.write(AppExit::Success);
}
