//! Старт браузерного клиента: скрыть HTML-оверлей, подтянуть digest TLS с сервера
//! статики, показать статус подключения (без нативного меню).

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::app_state::AppState;
use crate::config::{parse_digest_file, ClientConfig};
use crate::lynet::LyClient;
use crate::menu::{do_connect, ConnectError, ConnectTimeout};

/// Digest, полученный по HTTP из `/certificates/digest.txt` (обновляется сервером).
#[derive(Resource, Default)]
pub struct RuntimeCertDigest(pub Option<String>);

/// Максимум автопопыток переподключения подряд (потом — стартовый экран с
/// ошибкой; счётчик сбрасывается при входе в игру и по кнопке «Играть»).
const RECONNECT_MAX_ATTEMPTS: u32 = 30;

#[derive(Resource)]
struct WasmBootState {
    phase: WasmBootPhase,
    digest_slot: Arc<Mutex<Option<Result<String, String>>>>,
    fetch_started: bool,
    status: String,
    /// Номер текущей автопопытки переподключения (0 = обычный коннект).
    attempts: u32,
    /// Момент (Time::elapsed_secs_f64), раньше которого новую попытку не начинаем.
    retry_at: f64,
    /// nonce из `__cs2dStart` последней обработанной кнопки «Играть»: смена
    /// nonce = игрок нажал кнопку сам → счётчик попыток обнуляется.
    last_nonce: f64,
}

impl Default for WasmBootState {
    fn default() -> Self {
        Self {
            phase: WasmBootPhase::Idle,
            digest_slot: Arc::new(Mutex::new(None)),
            fetch_started: false,
            status: "Запуск…".into(),
            attempts: 0,
            retry_at: 0.0,
            last_nonce: 0.0,
        }
    }
}

#[derive(Default, PartialEq, Eq)]
enum WasmBootPhase {
    #[default]
    Idle,
    FetchingDigest,
    Done,
}

#[derive(Component)]
struct WasmStatusRoot;
#[derive(Component)]
struct WasmStatusText;
#[derive(Component)]
struct WasmUiCamera;

pub struct WasmBootPlugin;

impl Plugin for WasmBootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RuntimeCertDigest>()
            .init_resource::<WasmBootState>()
            .add_systems(Startup, (hide_html_loading, setup_wasm_status_ui))
            // При возврате из игры (обрыв) статус-UI пересоздаётся: его камера/
            // текст удаляются на входе в игру.
            .add_systems(OnEnter(AppState::Menu), (setup_wasm_status_ui, reset_wasm_boot))
            .add_systems(OnEnter(AppState::InGame), hide_wasm_status_ui)
            .add_observer(on_disconnected_ingame)
            .add_systems(
                Update,
                (
                    wasm_boot_tick.run_if(in_state(AppState::Menu)),
                    update_wasm_status_ui
                        .run_if(in_state(AppState::Menu).or(in_state(AppState::Connecting))),
                    wasm_heartbeat,
                    wasm_sync_probe,
                    wasm_move_probe,
                    wasm_warmup_probe,
                ),
            );
    }
}

/// Диагностический «пульс» (только wasm): раз в ~2 с пишет в консоль браузера
/// кадры/FPS, состояние, позиции игроков и СЕТЕВЫЕ метрики: RTT/джиттер линка,
/// тик локального таймлайна, скорость виртуального времени (адаптация синка)
/// и счётчик откатов предсказания. Если пульс идёт, а картинка статична —
/// мёртв только рендер; если пульса нет — встал весь event loop winit.
#[allow(clippy::too_many_arguments)]
fn wasm_heartbeat(
    time: Res<Time>,
    virt: Res<Time<Virtual>>,
    mut frames: Local<u64>,
    mut last: Local<f64>,
    mut last_frames: Local<u64>,
    state: Res<State<AppState>>,
    players: Query<(&netproto::Position, &netproto::Health), With<netproto::Player>>,
    predicted: Query<(), (With<netproto::Player>, With<lightyear::prelude::Predicted>)>,
    my: Res<crate::resources::MyPlayer>,
    cam: Query<&Transform, With<Camera2d>>,
    links: Query<&lightyear::prelude::Link, With<lightyear::prelude::Client>>,
    timeline: Res<lightyear::prelude::LocalTimeline>,
    metrics: Option<Res<lightyear::prediction::diagnostics::PredictionMetrics>>,
    aim: Res<crate::resources::AimAngle>,
    windows: Query<&Window>,
) {
    *frames += 1;
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    let fps = (*frames - *last_frames) as f64 / (now - *last).max(0.001);
    let p: Vec<String> = players
        .iter()
        .map(|(pos, hp)| format!("({:.0},{:.0})hp{}", pos.0.x, pos.0.y, hp.0))
        .collect();
    let c = cam
        .iter()
        .map(|tf| format!("({:.0},{:.0})", tf.translation.x, tf.translation.y))
        .collect::<Vec<_>>()
        .join(" ");
    let net = links
        .iter()
        .next()
        .map(|l| {
            format!(
                "rtt={:.0}ms jit={:.0}ms",
                l.stats.rtt.as_secs_f64() * 1000.0,
                l.stats.jitter.as_secs_f64() * 1000.0
            )
        })
        .unwrap_or_else(|| "rtt=?".into());
    let (rb, rbt) = metrics.map(|m| (m.rollbacks, m.rollback_ticks)).unwrap_or((0, 0));
    info!(
        "[hb] frame={} fps={:.1} state={:?} plr[{}] pred={} my_got={} cam[{}] {} tick={:?} speed={:.3} rollbacks={} rb_ticks={} aim={:.3}",
        *frames,
        fps,
        state.get(),
        p.join(" "),
        predicted.iter().count(),
        my.got,
        c,
        net,
        timeline.tick(),
        virt.relative_speed(),
        rb,
        rbt,
        aim.0,
    );
    // Диагностика маппинга курсора (масштаб экрана ≠ 100%): сырой курсор,
    // логический размер окна и scale_factor — сравнивать при разных dpr.
    if let Ok(w) = windows.single() {
        let cur = w
            .cursor_position()
            .map(|c| format!("({:.0},{:.0})", c.x, c.y))
            .unwrap_or_else(|| "-".into());
        info!(
            "[cur] raw={} logical={:.0}x{:.0} physical={}x{} sf={:.2}",
            cur,
            w.width(),
            w.height(),
            w.physical_width(),
            w.physical_height(),
            w.scale_factor()
        );
    }
    *last = now;
    *last_frames = *frames;
}

/// Готовность движка для лендинга — из РЕАЛЬНОГО состояния, не по таймерам:
/// 1) все стартовые ассеты загружены (аудио-пулы + музыка + шрифт — статус
///    каждого хэндла известен AssetServer'у точно); доля загруженного идёт в
///    `window.__cs2dProgress` (0..1) — из неё лендинг рисует полосу прогресса;
/// 2) сглаженный FPS держится ≥ 45 хотя бы секунду — прямое наблюдение, что
///    движок уже крутится плавно (JIT-дожатие wasm браузер никак не раскрывает,
///    но его эффект виден именно в FPS).
/// Оба условия выполнены → `window.__cs2dReady = true`, кнопка «ИГРАТЬ» оживает.
fn wasm_warmup_probe(
    time: Res<Time>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    asset_server: Res<AssetServer>,
    sfx: Option<Res<crate::systems::audio::Sfx>>,
    font: Option<Res<crate::resources::UiFont>>,
    mut done: Local<bool>,
    mut good_since: Local<f64>,
) {
    if *done {
        return;
    }
    let Some(win) = web_sys::window() else { return };

    // Точный статус ассетов: перечисляем все стартовые хэндлы.
    let mut total = 0usize;
    let mut loaded = 0usize;
    let mut count = |id: bevy::asset::UntypedAssetId| {
        total += 1;
        if asset_server.is_loaded_with_dependencies(id) {
            loaded += 1;
        }
    };
    if let Some(sfx) = sfx.as_deref() {
        for pool in [
            &sfx.growls, &sfx.attacks, &sfx.deaths, &sfx.flesh, &sfx.body,
            &sfx.block, &sfx.swing, &sfx.stun, &sfx.dash, &sfx.explosions,
            &sfx.music,
        ] {
            for h in pool {
                count(h.id().untyped());
            }
        }
    }
    if let Some(f) = font.as_deref() {
        count(f.0.id().untyped());
    }
    let assets_ready = total > 0 && loaded == total;
    let _ = js_sys::Reflect::set(
        &win,
        &wasm_bindgen::JsValue::from_str("__cs2dProgress"),
        &wasm_bindgen::JsValue::from_f64(if total == 0 { 0.0 } else { loaded as f64 / total as f64 }),
    );

    // FPS-условие: не эвристика по времени, а наблюдение фактической плавности.
    let now = time.elapsed_secs_f64();
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    if fps < 45.0 {
        *good_since = now;
    }
    let fps_ready = now - *good_since >= 1.0;

    if assets_ready && fps_ready {
        *done = true;
        let _ = js_sys::Reflect::set(
            &win,
            &wasm_bindgen::JsValue::from_str("__cs2dReady"),
            &wasm_bindgen::JsValue::TRUE,
        );
        info!("[wasm] готово: ассеты {loaded}/{total}, fps={fps:.0} (t={now:.1}с)");
    }
}

/// Диагностика синка Lightyear (пара к [hb], раз в ~2 с): применяемая ПРЯМО
/// СЕЙЧАС задержка ввода (delay), отрыв штампуемых вводов от последнего
/// услышанного серверного тика (ahead) и RTT/джиттер глазами самого синка
/// (EWMA PingManager; rtt в [hb] — их копия из Link.stats). Болезнь выглядит
/// так: delay в десятках-сотнях тиков при нормальном rtt — стартовый замер
/// попал в джанк страницы и «залип» (лечится refresh_input_delay в lynet.rs).
/// Норма: delay=3, ahead ≈ (rtt + 4·jit + 5мс)/15мс + delay.
fn wasm_sync_probe(
    time: Res<Time>,
    mut last: Local<f64>,
    local: Res<lightyear::prelude::LocalTimeline>,
    clients: Query<
        (
            &lightyear::prelude::client::InputTimeline,
            &lightyear::prelude::client::RemoteTimeline,
            &lightyear::prelude::PingManager,
            Has<lightyear::prelude::IsSynced<lightyear::prelude::client::InputTimeline>>,
        ),
        With<lightyear::prelude::Client>,
    >,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Ok((input, remote, ping, synced)) = clients.single() else {
        return;
    };
    // Тик, которым будут проштампованы вводы ЭТОГО кадра — ровно так же считает
    // buffer_action_state в lightyear_inputs (LocalTimeline + delay; сам
    // InputTimeline.tick() в Update неточен — обновляется в PostUpdate).
    let delay = input.input_delay();
    let input_tick = local.tick() + delay as i16;
    // Последний услышанный серверный тик (сглаженная remote-оценка наружу не
    // экспортируется в 0.26.4). Tick - Tick == i16 (wrapping), 1 тик = 15 мс.
    let ahead = remote.last_received_tick().map(|rt| input_tick - rt);
    info!(
        "[sync] delay={}t ahead={:?}t rtt={:.0}ms jit={:.0}ms pongs={} synced={}",
        delay,
        ahead,
        ping.rtt().as_secs_f64() * 1000.0,
        ping.jitter().as_secs_f64() * 1000.0,
        ping.pongs_recv,
        synced,
    );
}

/// Мгновенный лог движения предсказанного игрока (только wasm, для замера
/// задержки «нажатие → движение» через CDP): пишет позицию при сдвиге ≥ 2 юн.
/// от последней залогированной, не чаще ~10 раз/с.
fn wasm_move_probe(
    time: Res<Time>,
    mut last_pos: Local<Option<Vec2>>,
    mut last_log: Local<f64>,
    q: Query<&netproto::Position, (With<netproto::Player>, With<lightyear::prelude::Predicted>)>,
) {
    let Ok(pos) = q.single() else { return };
    let now = time.elapsed_secs_f64();
    let moved = last_pos.map_or(true, |p| p.distance(pos.0) >= 2.0);
    if moved && now - *last_log > 0.1 {
        info!("[mv] pred=({:.1},{:.1})", pos.0.x, pos.0.y);
        *last_pos = Some(pos.0);
        *last_log = now;
    }
}

fn hide_html_loading() {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Some(doc) = win.document() else {
        return;
    };
    if let Some(el) = doc.get_element_by_id("loading") {
        let _ = el.set_attribute("style", "display:none");
    }
}

/// Читает запрос старта со стартового HTML-экрана: `window.__cs2dStart =
/// {name, password, nonce}` выставляет кнопка «Играть» (см. index.html). Пока
/// объекта нет — игра ждёт (оверлей закрывает канвас). `nonce` меняется при
/// каждом клике — по нему авторетраи отличаются от ручного запуска.
fn js_start_request() -> Option<(String, String, f64)> {
    let win = web_sys::window()?;
    let val = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str("__cs2dStart")).ok()?;
    if !val.is_object() {
        return None;
    }
    let get = |k: &str| {
        js_sys::Reflect::get(&val, &wasm_bindgen::JsValue::from_str(k))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    };
    let nonce = js_sys::Reflect::get(&val, &wasm_bindgen::JsValue::from_str("nonce"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    Some((get("name"), get("password"), nonce))
}

/// Возврат на стартовый экран (ошибка авторизации/таймаут/выход в меню):
/// сбрасывает запрос старта и показывает HTML-оверлей с сообщением.
fn js_return_to_overlay(msg: Option<&str>) {
    let Some(win) = web_sys::window() else { return };
    let _ = js_sys::Reflect::set(
        &win,
        &wasm_bindgen::JsValue::from_str("__cs2dStart"),
        &wasm_bindgen::JsValue::UNDEFINED,
    );
    if let Ok(f) = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str("__cs2dShowOverlay")) {
        if let Some(f) = f.dyn_ref::<js_sys::Function>() {
            let _ = f.call1(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from_str(msg.unwrap_or("")),
            );
        }
    }
}

fn setup_wasm_status_ui(
    mut commands: Commands,
    assets: Res<AssetServer>,
    existing: Query<(), With<WasmStatusRoot>>,
) {
    // Идемпотентно: вызывается и на Startup, и при каждом возврате в Menu
    // (после игры UI удалён вместе с камерой).
    if !existing.is_empty() {
        return;
    }
    // Дефолтный шрифт Bevy без кириллицы — статус рисуем проектным шрифтом.
    let font = assets.load("fonts/FiraSans-Bold.ttf");
    commands.spawn((Camera2d::default(), WasmUiCamera));
    commands.spawn((
        WasmStatusRoot,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    ))
    .with_children(|root| {
        root.spawn((
            WasmStatusText,
            Text::new("Запуск…"),
            TextFont {
                font,
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::srgba(0.85, 0.85, 0.88, 1.0)),
        ));
    });
}

fn update_wasm_status_ui(
    boot: Res<WasmBootState>,
    err: Res<ConnectError>,
    state: Res<State<AppState>>,
    mut q: Query<&mut Text, With<WasmStatusText>>,
) {
    let msg = if boot.attempts > 0 {
        // идёт серия автопереподключений — err тут не финальный вердикт
        format!("Переподключение… (попытка {})", boot.attempts)
    } else if let Some(e) = err.0.as_ref() {
        e.clone()
    } else if matches!(state.get(), AppState::Connecting) {
        "Подключение к серверу…".to_string()
    } else {
        boot.status.clone()
    };
    if let Ok(mut t) = q.single_mut() {
        *t = Text::new(msg);
    }
}

/// Возврат в Menu. Три сценария:
/// - СЕТЕВАЯ ошибка (таймаут/обрыв) при живом запросе старта → авторетрай с
///   бэкоффом: клиент сам перефетчит digest и переподключится — игроку не нужно
///   ничего нажимать (сервер перезапустился → через пару секунд ты снова в игре);
/// - отказ авторизации (не тот пароль/сервер полон) → стартовый экран с ошибкой:
///   ретраить бессмысленно, нужен ввод пользователя;
/// - ручной выход/первый запуск (ошибки нет) → стартовый экран.
fn reset_wasm_boot(
    mut boot: ResMut<WasmBootState>,
    err: Res<ConnectError>,
    rejected: Res<crate::lynet::AuthRejected>,
    time: Res<Time>,
) {
    boot.phase = WasmBootPhase::Idle;
    if let Ok(mut slot) = boot.digest_slot.lock() {
        *slot = None;
    }
    boot.fetch_started = false;
    boot.status = String::new();

    let network_error = err.0.is_some() && !rejected.0;
    let can_retry = js_start_request().is_some() && boot.attempts < RECONNECT_MAX_ATTEMPTS;
    if network_error && can_retry {
        boot.attempts += 1;
        // бэкофф: 2с, 3с, 4с… потолок 8с — сервер после деплоя поднимается
        // за секунды, дольше ждать незачем
        let delay = (1.0 + boot.attempts as f64).min(8.0);
        boot.retry_at = time.elapsed_secs_f64() + delay;
        info!("[wasm] переподключение: попытка {} через {delay:.0}с", boot.attempts);
        return;
    }
    boot.attempts = 0;
    js_return_to_overlay(err.0.as_deref());
}

fn hide_wasm_status_ui(
    mut commands: Commands,
    mut boot: ResMut<WasmBootState>,
    q_root: Query<Entity, With<WasmStatusRoot>>,
    q_cam: Query<Entity, With<WasmUiCamera>>,
) {
    // успешный вход — серия автопереподключений закончена
    boot.attempts = 0;
    for e in q_root.iter().chain(q_cam.iter()) {
        commands.entity(e).despawn();
    }
}

/// Обрыв соединения ПОСРЕДИ игры (сервер перезапустился/упал): уходим в Menu с
/// сетевой ошибкой — там включится автопереподключение, и после подъёма сервера
/// игрок вернётся в бой сам, без F5 и повторного логина.
fn on_disconnected_ingame(
    trigger: On<Add, lightyear::prelude::Disconnected>,
    state: Res<State<AppState>>,
    mut err: ResMut<ConnectError>,
    mut next: ResMut<NextState<AppState>>,
) {
    let _ = trigger;
    if matches!(state.get(), AppState::InGame) {
        warn!("[wasm] соединение потеряно — автопереподключение");
        err.0 = Some("Соединение с сервером потеряно".into());
        next.set(AppState::Menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn wasm_boot_tick(
    mut boot: ResMut<WasmBootState>,
    mut cfg: ResMut<ClientConfig>,
    mut runtime_digest: ResMut<RuntimeCertDigest>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
    mut rejected: ResMut<crate::lynet::AuthRejected>,
    time: Res<Time>,
) {
    match boot.phase {
        WasmBootPhase::Idle => {
            if boot.fetch_started {
                return;
            }
            // Ждём кнопку «Играть» стартового экрана (index.html): она кладёт
            // имя/пароль в window.__cs2dStart. Заодно это жест пользователя —
            // AudioContext уже разбужен к моменту входа в игру.
            let Some((name, password, nonce)) = js_start_request() else {
                return;
            };
            // Свежий клик по кнопке (nonce сменился) сбрасывает авторетраи.
            if nonce != boot.last_nonce {
                boot.last_nonce = nonce;
                boot.attempts = 0;
                boot.retry_at = 0.0;
            }
            // Бэкофф между автопопытками переподключения.
            if time.elapsed_secs_f64() < boot.retry_at {
                boot.status = format!("Переподключение… (попытка {})", boot.attempts);
                return;
            }
            let name: String = name.trim().chars().take(24).collect();
            if !name.is_empty() {
                cfg.name = name;
            }
            cfg.password = password;

            boot.fetch_started = true;
            boot.status = format!("Читаем TLS digest…\n{}", cfg.connect_address());
            err.0 = None;
            rejected.0 = false;

            // Приоритет: ЯВНЫЙ override (#hash в URL / поле конфига) → свежий
            // HTTP-фетч → пусто (настоящий сертификат). В сборку digest не
            // вшивается: сервер генерит новый self-signed сертификат при КАЖДОМ
            // старте, и любое вшитое значение протухает при первом же рестарте.
            let preset = cfg.cert_digest.clone();
            if !preset.is_empty() {
                runtime_digest.0 = Some(preset);
                try_wasm_connect(
                    &cfg,
                    &mut boot,
                    &mut ly,
                    &mut next,
                    &mut commands,
                    &mut err,
                    &runtime_digest,
                );
                return;
            }

            boot.phase = WasmBootPhase::FetchingDigest;
            let slot = boot.digest_slot.clone();
            spawn_local(async move {
                let result = fetch_digest("/certificates/digest.txt").await;
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(result);
                }
            });
        }
        WasmBootPhase::FetchingDigest => {
            let result = boot
                .digest_slot
                .lock()
                .ok()
                .and_then(|mut g| g.take());
            let Some(result) = result else {
                boot.status = format!(
                    "Читаем TLS digest…\n{}\n\n(если сервер не запущен — запустите и F5)",
                    cfg.connect_address()
                );
                return;
            };
            match result {
                Ok(digest) if !digest.is_empty() => {
                    info!("[wasm] digest получен по HTTP: {}…", &digest[..digest.len().min(11)]);
                    runtime_digest.0 = Some(digest);
                    try_wasm_connect(
                        &cfg,
                        &mut boot,
                        &mut ly,
                        &mut next,
                        &mut commands,
                        &mut err,
                        &runtime_digest,
                    );
                }
                Ok(_) | Err(_) => {
                    // Фетч не удался/пуст → подключаемся с ПУСТЫМ digest: прод-
                    // вариант, где digest.txt на статике намеренно отсутствует и
                    // браузер валидирует НАСТОЯЩИЙ сертификат (WebPKI). Никаких
                    // «вшитых» фолбэков: вшитое значение — это случайный локальный
                    // файл на момент сборки, оно молча давало протухший digest и
                    // CERTIFICATE_VERIFY_FAILED, который невозможно диагностировать.
                    if let Err(e) = &result {
                        warn!("[wasm] digest.txt недоступен ({e}) — пробуем настоящий TLS-сертификат");
                    } else {
                        info!("[wasm] digest.txt пуст — подключаемся по настоящему TLS-сертификату (prod)");
                    }
                    runtime_digest.0 = Some(String::new());
                    try_wasm_connect(
                        &cfg,
                        &mut boot,
                        &mut ly,
                        &mut next,
                        &mut commands,
                        &mut err,
                        &runtime_digest,
                    );
                }
            }
        }
        WasmBootPhase::Done => {}
    }
}

fn try_wasm_connect(
    cfg: &ClientConfig,
    boot: &mut WasmBootState,
    ly: &mut LyClient,
    next: &mut NextState<AppState>,
    commands: &mut Commands,
    err: &mut ConnectError,
    runtime_digest: &RuntimeCertDigest,
) {
    let addr = cfg.connect_address();
    let digest = runtime_digest.0.as_deref();
    match do_connect(&addr, commands, ly, cfg, digest) {
        Ok(_) => {
            err.0 = None;
            boot.phase = WasmBootPhase::Done;
            boot.status = "Подключение…".into();
            commands.insert_resource(ConnectTimeout(Timer::from_seconds(12.0, TimerMode::Once)));
            next.set(AppState::Connecting);
        }
        Err(e) => {
            boot.phase = WasmBootPhase::Idle;
            boot.fetch_started = false;
            err.0 = Some(format!("Сервер не найден: {e}"));
        }
    }
}

async fn fetch_digest(url: &str) -> Result<String, String> {
    use wasm_bindgen_futures::JsFuture;
    let win = web_sys::window().ok_or("нет window")?;
    // Digest меняется при каждом рестарте сервера, а fetch без предосторожностей
    // отдаёт закэшированный файл (Ctrl+F5 перегружает страницу, но кэш для
    // fetch() из wasm НЕ сбрасывает) → клиент коннектился со старым digest и
    // ловил CERTIFICATE_VERIFY_FAILED. Двойная защита: cache-buster в URL
    // (пробивает ЛЮБОЙ слой кэша, включая промежуточные) + cache: no-store.
    let url = format!("{url}?t={}", crate::platform::now_nanos());
    let opts = web_sys::RequestInit::new();
    opts.set_cache(web_sys::RequestCache::NoStore);
    let req = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|_| "bad request".to_string())?;
    let resp_val = JsFuture::from(win.fetch_with_request(&req))
        .await
        .map_err(|_| "fetch await failed".to_string())?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| "не Response".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text_val = JsFuture::from(
        resp.text()
            .map_err(|_| "text() failed".to_string())?,
    )
    .await
    .map_err(|_| "text await failed".to_string())?;
    let text = text_val.as_string().unwrap_or_default();
    let digest = parse_digest_file(&text);
    if digest.is_empty() {
        Err("digest пустой".into())
    } else {
        Ok(digest)
    }
}
