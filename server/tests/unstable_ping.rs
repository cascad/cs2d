//! Нестабильный пинг по ходу сессии: моментами дерьмовый, потом снова нормальный.
//! Играть после восстановления должно быть нормально БЕЗ пересоздания соединения.
//!
//! Баг-храповик lightyear 0.26.4 (см. netproto/src/client_sync.rs): задержка
//! ввода пересчитывается только при жёстком ресинке, а завышенная задержка сама
//! гасит сигнал ресинка (кламп sync_objective на remote+1) — просадка сети (или
//! джанк страницы в окне рукопожатия) замораживает многосекундную задержку до
//! конца сессии, хотя EWMA-оценка RTT выздоравливает за секунды.
//!
//! Здесь НАСТОЯЩИЙ серверный бинарь (CARGO_BIN_EXE_server, cargo собирает сам)
//! и клиентское приложение по рецепту netproto/examples/ly_client (проверен
//! E2E). Плохая сеть эмулируется штатным LinkConditioner на приёме клиента:
//! фаза 1 — здоровая сеть (RTT ~30 мс), фаза 2 — просадка (RTT ~1.8 с),
//! фаза 3 — сеть снова здоровая. Проверяем:
//! - `ratchet_canary_stuck_without_refresh`: БЕЗ обхода задержка остаётся
//!   завышенной (канарейка: если после апгрейда lightyear тест упадёт — баг
//!   починили upstream, обход `refresh_input_delay` можно снимать);
//! - `unstable_ping_recovers_with_refresh`: с обходом задержка возвращается к
//!   норме за секунды, и реакция мира на свежий ввод снова быстрая.

use core::net::{Ipv4Addr, SocketAddr};
use core::time::Duration;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use bevy::prelude::*;
use lightyear::link::RecvLinkConditioner;
use lightyear::netcode::NetcodeClient;
use lightyear::netcode::client_plugin::NetcodeConfig;
use lightyear::prelude::client::input::*;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::*;
use lightyear::prelude::*;

use netproto::{
    AuthChannel, Hello, InputDelayPolicy, NetInput, PRIVATE_KEY, PROTOCOL_ID, Player,
    ProtocolPlugin, Rotation, tick_duration,
};

const GOOD: (u64, u64) = (15, 3); // (латентность, джиттер) мс на приём — RTT ~30 мс
const SPIKE: (u64, u64) = (900, 80); // просадка: RTT ~1.8 с

fn conditioner(cfg: (u64, u64)) -> RecvLinkConditioner {
    RecvLinkConditioner::new(LinkConditionerConfig {
        incoming_latency: Duration::from_millis(cfg.0),
        incoming_jitter: Duration::from_millis(cfg.1),
        incoming_loss: 0.0,
    })
}

fn test_delay_cfg() -> InputDelayConfig {
    // Прежний wasm-конфиг {2,3,7}: просадка сети конвертируется в задержку
    // ввода агрессивнее — храповик виден выпукло.
    InputDelayConfig {
        minimum_input_delay_ticks: 2,
        maximum_input_delay_before_prediction: 3,
        maximum_predicted_ticks: 7,
    }
}

fn test_sync_cfg() -> SyncConfig {
    SyncConfig {
        handshake_pings: 8,
        ..Default::default()
    }
}

/// Реальный серверный бинарь во временном каталоге (стейт/сертификаты — туда же).
/// Drop гасит процесс даже при панике теста.
struct ServerProc {
    child: Child,
    dir: std::path::PathBuf,
}

impl Drop for ServerProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server(port: u16, wt_port: u16) -> ServerProc {
    let dir = std::env::temp_dir().join(format!("cs2d_unstable_ping_{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cfg_path = dir.join("server_config.toml");
    std::fs::write(
        &cfg_path,
        format!(
            "ip = \"127.0.0.1\"\nport = {port}\nwebtransport_port = {wt_port}\n\
             name = \"unstable-ping-test\"\nmax_players = 4\nafk_kick_secs = 0.0\n"
        ),
    )
    .expect("write config");
    let log = std::fs::File::create(dir.join("server.log")).expect("log file");
    let child = Command::new(env!("CARGO_BIN_EXE_server"))
        .arg("--config")
        .arg(&cfg_path)
        .current_dir(&dir)
        .stdout(Stdio::from(log.try_clone().expect("log clone")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("spawn server");
    // Мета-эндпоинт лобби слушает TCP на игровом порту — годится как проба
    // готовности (UDP-листенер поднимается тем же Startup).
    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "сервер не поднялся за 30с");
        std::thread::sleep(Duration::from_millis(100));
    }
    ServerProc { child, dir }
}

/// Желаемый ввод клиента: тест переключает, buffer_test_input пишет в ActionState.
#[derive(Resource, Default, Clone, Copy)]
struct TestInput(NetInput);

fn buffer_test_input(
    want: Res<TestInput>,
    mut q: Query<&mut ActionState<NetInput>, With<InputMarker<NetInput>>>,
) {
    if let Ok(mut action) = q.single_mut() {
        action.0 = want.0;
    }
}

/// Появилась предсказанная копия нашего игрока — вешаем InputMarker (как в
/// ly_client), чтобы buffer_test_input писал ввод именно в неё.
fn handle_predicted_spawn(
    trigger: On<Add, Player>,
    predicted: Query<(), With<Predicted>>,
    mut commands: Commands,
) {
    if predicted.get(trigger.entity).is_ok() {
        commands
            .entity(trigger.entity)
            .insert(InputMarker::<NetInput>::default());
    }
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
            name: "PingTest".into(),
            password: "pw".into(),
        });
        *sent = true;
    }
}

/// Клиент по рецепту ly_client + кондиционер + та же политика синка/задержки,
/// что в client/src/lynet.rs.
fn build_client(port: u16, client_id: u64, with_refresh: bool) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(client::ClientPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    app.init_resource::<TestInput>();
    app.add_systems(
        FixedPreUpdate,
        buffer_test_input.in_set(InputSystems::WriteClientInputs),
    );
    app.add_systems(Update, send_hello);
    app.add_observer(handle_predicted_spawn);
    if with_refresh {
        app.insert_resource(InputDelayPolicy::new(test_delay_cfg(), test_sync_cfg()));
        app.add_systems(Update, netproto::refresh_input_delay);
    }

    // Спавн и Connect — в Startup: до первого update() наблюдатели lightyear
    // ещё не зарегистрированы и триггер ушёл бы в пустоту.
    app.add_systems(Startup, move |mut commands: Commands| {
        let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
        let netcode = NetcodeClient::new(
            Authentication::Manual {
                server_addr,
                client_id,
                private_key: PRIVATE_KEY,
                protocol_id: PROTOCOL_ID,
            },
            NetcodeConfig {
                client_timeout_secs: 10,
                ..default()
            },
        )
        .expect("netcode client");
        let entity = commands
            .spawn((
                Client::default(),
                Link::new(Some(conditioner(GOOD))),
                LocalAddr(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)),
                PeerAddr(server_addr),
                ReplicationReceiver::default(),
                PredictionManager::default(),
                InputTimelineConfig::new(test_sync_cfg(), test_delay_cfg()),
                // Как в client/src/lynet.rs: пинги каждые 50 мс — EWMA-оценка
                // RTT после просадки выздоравливает вдвое быстрее дефолта.
                PingManager::new(PingConfig {
                    ping_interval: Duration::from_millis(50),
                }),
                netcode,
                UdpIo::default(),
            ))
            .id();
        commands.trigger(Connect { entity });
    });
    app
}

fn client_delay_ticks(client: &mut App) -> Option<u16> {
    let world = client.world_mut();
    let mut q = world.query_filtered::<&InputTimeline, With<Client>>();
    q.single(world).ok().map(|t| t.input_delay())
}

fn set_conditioner(client: &mut App, cfg: (u64, u64)) {
    let world = client.world_mut();
    let mut q = world.query_filtered::<&mut Link, With<Client>>();
    if let Ok(mut link) = q.single_mut(world) {
        link.recv.conditioner = Some(conditioner(cfg));
    }
}

/// Поворот ПОДТВЕРЖДЁННОЙ копии игрока — серверное состояние, приехавшее по
/// репликации: реакция всего круга «ввод → сервер → репликация обратно».
/// На confirmed-сущности реплицируемый компонент лежит как `Confirmed<T>`.
fn confirmed_rotation(client: &mut App) -> Option<f32> {
    let world = client.world_mut();
    let mut q = world.query::<&Confirmed<Rotation>>();
    if let Some(r) = q.iter(world).next() {
        return Some(r.0.0);
    }
    let mut q2 = world.query_filtered::<&Rotation, (With<Player>, Without<Predicted>)>();
    q2.iter(world).next().map(|r| r.0)
}

/// Прогон сценария «здоровая сеть → просадка → восстановление». Возвращает
/// (задержка после синка, максимум за просадку, задержка в конце,
///  реакция мира на свежий ввод после восстановления).
fn run_scenario(port: u16, client_id: u64, name: &str, with_refresh: bool) -> (u16, u16, u16, Duration) {
    let _server = start_server(port, port + 10);
    let mut client = build_client(port, client_id, with_refresh);
    // Ручной update() НЕ вызывает finish/cleanup плагинов (это делает runner
    // внутри app.run()) — а lightyear доделывает проводку сообщений именно там.
    // Без этого netcode-хендшейк проходит, но ни одно сообщение не ходит.
    while client.plugins_state() == bevy::app::PluginsState::Adding {
        std::thread::sleep(Duration::from_millis(1));
    }
    client.finish();
    client.cleanup();

    let start = Instant::now();
    let mut healthy_delay = None::<u16>;
    let mut spike_max = 0u16;
    let mut spiked = false;
    let mut recovered = false;
    let mut aim_flip: Option<Instant> = None;
    let mut react_latency = None::<Duration>;
    let mut last_dbg = 0u64;

    // 20 секунд реального времени: 0-3 здоровая, 3-6 просадка, 6-20 восстановление.
    loop {
        let t = start.elapsed().as_secs_f64();
        client.update();
        std::thread::sleep(Duration::from_millis(2));

        let delay = client_delay_ticks(&mut client).unwrap_or(0);
        if t > 2.5 && t < 3.0 && healthy_delay.is_none() {
            healthy_delay = Some(delay);
        }
        if t >= 3.0 && !spiked {
            spiked = true;
            set_conditioner(&mut client, SPIKE);
        }
        if t >= 3.0 && t < 6.5 {
            spike_max = spike_max.max(delay);
        }
        if t >= 6.0 && !recovered {
            recovered = true;
            set_conditioner(&mut client, GOOD);
        }
        // Реакция мира на свежий ввод после восстановления: на 16-й секунде
        // крутим прицел и ждём, когда ПОДТВЕРЖДЁННЫЙ поворот заметно изменится
        // (доставка ввода + серверный шаг + репликация обратно).
        if t >= 16.0 && aim_flip.is_none() {
            client.world_mut().resource_mut::<TestInput>().0.aim = 2.5;
            aim_flip = Some(Instant::now());
        }
        if let (Some(flip), None) = (aim_flip, react_latency) {
            if let Some(rot) = confirmed_rotation(&mut client) {
                if rot.abs() > 0.1 {
                    react_latency = Some(flip.elapsed());
                }
            }
        }
        if t as u64 > last_dbg {
            last_dbg = t as u64;
            eprintln!("[{name}] t={t:.0} delay={delay}t rot={:?}", confirmed_rotation(&mut client));
        }
        if t >= 20.0 {
            break;
        }
    }

    let final_delay = client_delay_ticks(&mut client).unwrap_or(u16::MAX);
    let lat = react_latency.unwrap_or(Duration::from_secs(99));
    let healthy = healthy_delay.unwrap_or(u16::MAX);
    eprintln!(
        "[{name}] healthy={healthy}t spike_max={spike_max}t final={final_delay}t react={lat:?}"
    );
    (healthy, spike_max, final_delay, lat)
}

/// Канарейка: БЕЗ обхода задержка ввода после просадки остаётся завышенной до
/// конца сессии (upstream-баг). Если после апгрейда lightyear тест упадёт на
/// последнем assert — храповик починили, обход refresh_input_delay можно снимать.
#[test]
fn ratchet_canary_stuck_without_refresh() {
    let (healthy, spike_max, final_delay, _lat) = run_scenario(6161, 61, "canary", false);
    assert!(healthy <= 3, "после синка на здоровой сети делей {healthy} > 3");
    assert!(
        spike_max >= 20,
        "просадка не отравила делей (max {spike_max}) — сценарий не воспроизвёл баг"
    );
    assert!(
        final_delay > healthy + 2,
        "делей вернулся к норме БЕЗ обхода ({final_delay}т) — похоже, храповик \
         починили upstream: канарейка своё отслужила, обход можно снимать"
    );
}

/// С обходом: после восстановления сети задержка возвращается к норме за
/// секунды, мир снова быстро реагирует на ввод — играть можно, реконнекта нет.
#[test]
fn unstable_ping_recovers_with_refresh() {
    let (healthy, spike_max, final_delay, lat) = run_scenario(6162, 62, "refresh", true);
    assert!(healthy <= 3, "после синка на здоровой сети делей {healthy} > 3");
    assert!(
        spike_max >= 20,
        "просадка не отравила делей (max {spike_max}) — сценарий не воспроизвёл баг"
    );
    assert!(
        final_delay <= 5,
        "делей не вернулся к норме с обходом: {final_delay} тиков"
    );
    assert!(
        lat < Duration::from_millis(400),
        "мир отреагировал на свежий ввод через {lat:?} (> 400 мс) — играть всё ещё нельзя"
    );
}
