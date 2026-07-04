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

#[derive(Resource)]
struct WasmBootState {
    phase: WasmBootPhase,
    digest_slot: Arc<Mutex<Option<Result<String, String>>>>,
    fetch_started: bool,
    status: String,
}

impl Default for WasmBootState {
    fn default() -> Self {
        Self {
            phase: WasmBootPhase::Idle,
            digest_slot: Arc::new(Mutex::new(None)),
            fetch_started: false,
            status: "Запуск…".into(),
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
            .add_systems(OnEnter(AppState::Menu), reset_wasm_boot)
            .add_systems(OnEnter(AppState::InGame), hide_wasm_status_ui)
            .add_systems(
                Update,
                (
                    wasm_boot_tick.run_if(in_state(AppState::Menu)),
                    update_wasm_status_ui
                        .run_if(in_state(AppState::Menu).or(in_state(AppState::Connecting))),
                    wasm_heartbeat,
                    wasm_move_probe,
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
        "[hb] frame={} fps={:.1} state={:?} plr[{}] pred={} my_got={} cam[{}] {} tick={:?} speed={:.3} rollbacks={} rb_ticks={}",
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
    );
    *last = now;
    *last_frames = *frames;
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

fn setup_wasm_status_ui(mut commands: Commands, assets: Res<AssetServer>) {
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
    let msg = if let Some(e) = err.0.as_ref() {
        format!("{e}\n\nПроверьте, что сервер запущен:\ncargo run -p server")
    } else if matches!(state.get(), AppState::Connecting) {
        "Подключение к серверу…".to_string()
    } else {
        boot.status.clone()
    };
    if let Ok(mut t) = q.single_mut() {
        *t = Text::new(msg);
    }
}

fn reset_wasm_boot(mut boot: ResMut<WasmBootState>) {
    boot.phase = WasmBootPhase::Idle;
    if let Ok(mut slot) = boot.digest_slot.lock() {
        *slot = None;
    }
    boot.fetch_started = false;
    boot.status = "Запуск…".into();
}

fn hide_wasm_status_ui(
    mut commands: Commands,
    q_root: Query<Entity, With<WasmStatusRoot>>,
    q_cam: Query<Entity, With<WasmUiCamera>>,
) {
    for e in q_root.iter().chain(q_cam.iter()) {
        commands.entity(e).despawn();
    }
}

fn wasm_boot_tick(
    mut boot: ResMut<WasmBootState>,
    cfg: Res<ClientConfig>,
    mut runtime_digest: ResMut<RuntimeCertDigest>,
    mut ly: ResMut<LyClient>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    mut err: ResMut<ConnectError>,
) {
    match boot.phase {
        WasmBootPhase::Idle => {
            if boot.fetch_started {
                return;
            }
            boot.fetch_started = true;
            boot.status = format!("Читаем TLS digest…\n{}", cfg.connect_address());
            err.0 = None;

            // Приоритет: ЯВНЫЙ override (#hash в URL / поле конфига) → свежий
            // HTTP-фетч → вшитый при сборке. Вшитый digest НЕ короткое
            // замыкание: сервер генерит новый self-signed сертификат при КАЖДОМ
            // старте, и вшитое значение протухает при первом же рестарте
            // сервера (браузер молча ловил TLS-отказ до пересборки wasm).
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
                    // Фетч не удался/пуст. Запасные варианты по порядку:
                    // 1) вшитый при сборке digest (dev: может быть протухшим);
                    // 2) ПУСТОЙ digest — прод: на статике digest.txt намеренно
                    //    отсутствует, браузер валидирует НАСТОЯЩИЙ сертификат
                    //    (Let's Encrypt) через WebPKI. Хеши тут и не сработали
                    //    бы: Chrome принимает serverCertificateHashes только для
                    //    сертификатов со сроком жизни ≤ 2 недель.
                    let embedded = cfg.effective_cert_digest();
                    if !embedded.is_empty() {
                        warn!("[wasm] digest.txt недоступен, используем вшитый (может не совпасть с сервером)");
                        runtime_digest.0 = Some(embedded);
                    } else {
                        info!("[wasm] digest нет нигде — подключаемся по настоящему TLS-сертификату (prod)");
                        runtime_digest.0 = Some(String::new());
                    }
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
    // cache: no-store ОБЯЗАТЕЛЕН: digest меняется при каждом рестарте сервера,
    // а обычный fetch отдаёт закэшированный файл (Ctrl+F5 страницу перегружает,
    // но кэш для fetch() из wasm НЕ сбрасывает) — клиент коннектился со старым
    // digest и ловил CERTIFICATE_VERIFY_FAILED до чистки кэша.
    let opts = web_sys::RequestInit::new();
    opts.set_cache(web_sys::RequestCache::NoStore);
    let req = web_sys::Request::new_with_str_and_init(url, &opts)
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
