//! Stage 4: клиент на Lightyear с предсказанием. Подключается, получает свою
//! предсказанную сущность, в FixedPreUpdate буферизует ввод (headless: «жмём
//! вправо»), в FixedUpdate двигает предсказанного игрока тем же `step_player`,
//! что и сервер. Видно, что предсказанная позиция растёт каждый тик (мгновенный
//! отклик), а сервер подтверждает её без откатов.
//! Запуск: `cargo run -p netproto --example ly_client`.

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
    AbilityState, AuthChannel, AuthDenied, AuthOk, Blocking, Grenade, Health, Hello, MapGrids,
    NetInput, Npc, NpcRuntime, PRIVATE_KEY, PROTOCOL_ID, Player, Position, ProtocolPlugin, Rotation,
    SERVER_PORT, Scoreboard, server_cfg, step_player, tick_duration,
};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 120.0))),
    );
    app.add_plugins(LogPlugin::default());
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
        (send_hello, recv_auth, recv_scoreboard, log_positions, log_npc_contract),
    );
    app.add_observer(recv_fx);
    app.add_observer(on_connected);
    app.add_observer(on_disconnected);
    app.add_observer(handle_predicted_spawn);
    app.run();
}

/// После успешного коннекта один раз шлём `Hello` (логин+пароль) по каналу
/// авторизации. Важно слать только когда линк уже `Connected` — иначе канал ещё
/// не заведён на транспорте.
fn send_hello(
    mut sent: Local<bool>,
    mut sender: Query<&mut MessageSender<Hello>, With<Connected>>,
) {
    if *sent {
        return;
    }
    if let Ok(mut s) = sender.single_mut() {
        s.send::<AuthChannel>(Hello {
            name: "Bob".into(),
            password: "pw".into(),
        });
        *sent = true;
        info!("sent Hello (name=Bob)");
    }
}

/// Логируем ответ авторизации сервера.
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

/// Логируем разовые FX-события сервера (бой/взрыв/смерть/звук неписи).
fn recv_fx(trigger: On<RemoteEvent<Fx>>) {
    info!("FX: {:?}", trigger.event().trigger);
}

/// Логируем таблицу очков.
fn recv_scoreboard(mut sb: Query<&mut MessageReceiver<Scoreboard>>) {
    for mut r in &mut sb {
        for board in r.receive() {
            let rows: Vec<String> = board
                .0
                .iter()
                .map(|e| format!("{}: {}/{}/{} online={}", e.name, e.kills, e.npc_kills, e.deaths, e.online))
                .collect();
            info!("SCOREBOARD: [{}]", rows.join(", "));
        }
    }
}

fn connect_client(mut commands: Commands) {
    let client_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0);
    let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), SERVER_PORT);
    let auth = Authentication::Manual {
        server_addr,
        client_id: 1,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 3,
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
            UdpIo::default(),
        ))
        .id();
    commands.trigger(Connect { entity });
    info!("client connecting to {server_addr} ...");
}

/// Записываем ввод в `ActionState` управляемого игрока (в наборе WriteClientInputs).
/// Stage 10b (headless): машем мечом (Combat melee FX) и один раз бросаем гранату
/// в чейзера (Detonation + возможный NpcDeath FX), чтобы проверить FX-канал.
fn buffer_input(
    time: Res<Time>,
    mut elapsed: Local<f32>,
    mut q: Query<&mut ActionState<NetInput>, With<InputMarker<NetInput>>>,
) {
    *elapsed += time.delta_secs();
    if let Ok(mut action) = q.single_mut() {
        // Идём влево по нижнему коридору (y≈-430) к скелету 115 @ (-16,-432),
        // чтобы проверить ближний круг видимости на НАСТОЯЩЕМ сервере.
        action.0 = NetInput {
            aim: 0.0,
            ..default()
        };
    }
}

/// Двигаем ТОЛЬКО предсказанного игрока (которым владеем) тем же шагом, что сервер
/// (включая скольжение вдоль стен из общей карты).
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

/// КОНТРАКТ ДАННЫХ для клиентского «моста» неписей. Реальный клиент навешивает
/// визуал по `(Npc, Position, Rotation, Interpolated)` и синхронит по
/// `(Position, Rotation, Health, NpcRuntime)`. Если хоть одного компонента нет на
/// интерполируемой сущности — непись «есть, но не видно». Здесь это проверяем
/// headless и печатаем дефицит покомпонентно.
#[allow(clippy::type_complexity)]
fn log_npc_contract(
    time: Res<Time>,
    mut acc: Local<f32>,
    any_npc: Query<Entity, With<Npc>>,
    any_interp: Query<Entity, With<Interpolated>>,
    attach_ok: Query<Entity, (With<Npc>, With<Position>, With<Rotation>, With<Interpolated>)>,
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
    has_npc: Query<&Npc, With<Interpolated>>,
    has_rot: Query<Entity, (With<Npc>, With<Rotation>, With<Interpolated>)>,
    has_hp: Query<Entity, (With<Npc>, With<Health>, With<Interpolated>)>,
    has_rt: Query<Entity, (With<Npc>, With<NpcRuntime>, With<Interpolated>)>,
) {
    *acc += time.delta_secs();
    if *acc < 0.5 {
        return;
    }
    *acc = 0.0;
    info!(
        "NPC CONTRACT: any_npc={} any_interp={} | interp_npc={} +Rot={} +Health={} +NpcRuntime={} | attach_ok={} sync_ok={}",
        any_npc.iter().count(),
        any_interp.iter().count(),
        has_npc.iter().count(),
        has_rot.iter().count(),
        has_hp.iter().count(),
        has_rt.iter().count(),
        attach_ok.iter().count(),
        sync_ok.iter().count(),
    );
}

fn on_connected(trigger: On<Add, Connected>) {
    info!("CONNECTED to server: {:?}", trigger.entity);
}

fn on_disconnected(trigger: On<Add, Disconnected>) {
    info!("DISCONNECTED: {:?}", trigger.entity);
}

/// Появилась предсказанная копия НАШЕГО игрока — вешаем `InputMarker`, чтобы
/// `buffer_input` писал ввод именно в неё.
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

/// Раз в секунду печатаем предсказанную позицию игрока и реплицированных неписей
/// (видим, что NPC приезжают/уезжают и сколько их сейчас на клиенте).
fn log_positions(
    time: Res<Time>,
    mut acc: Local<f32>,
    me: Query<&Position, With<Predicted>>,
    // Неписи на ИНТЕРПОЛИРУЕМЫХ сущностях несут маркер Npc и Health (зарегистрированы
    // с интерполяцией) — именно их читает клиентский мост в реальном приложении.
    npcs: Query<(&Position, &netproto::Health), (With<Npc>, With<Interpolated>)>,
    grenades: Query<&Position, With<Grenade>>,
) {
    *acc += time.delta_secs();
    if *acc < 0.25 {
        return;
    }
    *acc = 0.0;
    for pos in &me {
        info!("CLIENT predicted Position = ({:.1}, {:.1})", pos.0.x, pos.0.y);
    }
    let npos: Vec<String> = npcs
        .iter()
        .map(|(p, hp)| format!("({:.0},{:.0})hp{}", p.0.x, p.0.y, hp.0))
        .collect();
    info!("npcs visible = {}: {}", npos.len(), npos.join(" "));
    let gpos: Vec<String> = grenades
        .iter()
        .map(|p| format!("({:.0},{:.0})", p.0.x, p.0.y))
        .collect();
    if !gpos.is_empty() {
        info!("grenades visible = {}: {}", gpos.len(), gpos.join(" "));
    }
}
