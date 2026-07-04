//! Клиентский сетевой слой на Lightyear 0.26 (замена самописного quinnet-стека).
//!
//! Здесь: подключение/авторизация, запись пользовательского ввода в нативный
//! `ActionState<NetInput>`, локальное предсказание движения тем же `step_player`,
//! что и сервер, а также «мост» к существующей отрисовке/звуку/UI: на
//! реплицируемые сущности (предсказанный локальный игрок / интерполируемые
//! чужие игроки, неписи, гранаты) навешиваются прежние визуальные компоненты
//! (`WorldPos`, `Facing`, `ActorAnim`, спрайты, кольцо, HP-бар), а одноразовые
//! эффекты сервера (`Fx`) транслируются в уже существующие клиентские события.

use std::collections::HashMap;

use core::net::{Ipv4Addr, SocketAddr};
use core::time::Duration;

use bevy::prelude::*;
use lightyear::interpolation::plugin::InterpolationSystems;
use lightyear::interpolation::timeline::InterpolationConfig;
use lightyear::netcode::NetcodeClient;
use lightyear::netcode::client_plugin::NetcodeConfig;
use lightyear::prelude::client::{InputDelayConfig, InputTimelineConfig};
use lightyear::prelude::client::input::*;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::*;
use lightyear::prelude::*;

use netproto::messages::{DeathKind, Fx, FxKind};
use netproto::{
    AbilityState, AuthChannel, AuthDenied, AuthOk, Blocking, Health as NetHealth, Hello, MapGrids,
    NetInput, Npc, NpcRuntime, Player, Position, Rotation, Scoreboard, Grenade as NetGrenade,
    PRIVATE_KEY, PROTOCOL_ID, server_cfg, step_player,
};

use crate::app_state::AppState;
use crate::components::{
    ActorAnim, AnimState, Corpse, DirNotch, Facing, LocalPlayer, NpcAnim, NpcHpFill, NpcKindC,
    NpcMarker, PlayerMarker,
};
use crate::config::ClientConfig;
use crate::events::{
    CombatSfxEvent, CombatSfxKind, GrenadeDetonatedEvent, NpcDiedEvent, NpcSoundEvent,
    PlayerDamagedEvent,
};
use crate::render::{layers, world_to_translation, RenderLayer, WorldPos};
use crate::resources::{
    Corpses, HpUiMap, LocalAbilities, LocalStatus, MyPlayer, ScoreboardData,
};
use crate::systems::iso::{KnightAnims, ISO_ACTOR_PX, KNIGHT_ANCHOR_Y, knight_row, KNIGHT_COLS};
use crate::systems::npc::{
    NpcAnims, NpcInfo, NpcStun, nearest_npc, npc_sprite_layout, spawn_npc_corpse,
};
use crate::systems::utils::spawn_hp_ui;

// ============================================================================
// Ресурсы сетевого слоя
// ============================================================================

/// Сущность-клиент Lightyear (для отключения/очистки между сессиями).
#[derive(Resource, Default)]
pub struct LyClient {
    pub entity: Option<Entity>,
}

/// Уже отправили `Hello` для текущего подключения?
#[derive(Resource, Default)]
pub struct HelloSent(pub bool);

/// Разовые намерения ввода, пойманные в `Update` (могут случиться в кадре, где
/// `FixedPreUpdate` не выполняется) и потребляемые при записи `NetInput`.
#[derive(Resource, Default)]
pub struct PendingDiscrete {
    pub dash: bool,
    pub stun: bool,
    pub throw: Option<Vec2>,
}

/// Предыдущее HP игроков/неписей — чтобы порождать события урона (вспышка/звук/
/// полоска), которых в новом протоколе нет отдельным сообщением.
#[derive(Resource, Default)]
pub struct PrevHp(pub HashMap<u64, i32>);

/// Маркер визуальной гранаты (на реплицируемой сущности `NetGrenade`).
#[derive(Component)]
pub struct GrenadeViz;

/// Читает реплицируемый компонент: на интерполируемых сущностях Lightyear
/// держит его как `Confirmed<T>`, а «живой» `T` появляется позже.
fn read_comp<T: Clone>(plain: Option<&T>, confirmed: Option<&Confirmed<T>>) -> Option<T> {
    plain.cloned().or_else(|| confirmed.map(|c| c.0.clone()))
}

/// Читает СВЕЖЕЕ значение реплицируемого компонента: `Confirmed<T>` в приоритете.
/// «Живой» `T` на интерполируемых сущностях обновляется интерполяционной
/// таймлинией с задержкой (а редко меняющиеся значения вроде HP могут ждать
/// следующего снапшота), тогда как `Confirmed<T>` — сразу по приёму пакета.
/// Для ДИСКРЕТНОГО состояния (HP, флаги атаки/агра) задержка не нужна.
fn read_fresh<T: Clone>(plain: Option<&T>, confirmed: Option<&Confirmed<T>>) -> Option<T> {
    confirmed.map(|c| c.0.clone()).or_else(|| plain.cloned())
}

/// Визуальное сглаживание позиции: экспоненциальный догон цели (~30 мс полужизни)
/// маскирует ступеньки тикрейта/снапшотов; большой скачок (телепорт/респаун/
/// коррекция) — мгновенный снап, чтобы не «плыть» через полкарты.
const SMOOTH_SNAP_DIST: f32 = 150.0;
const SMOOTH_HALF_LIFE: f32 = 0.03;

#[inline]
fn smooth_pos(current: Vec2, target: Vec2, dt: f32) -> Vec2 {
    if current.distance_squared(target) > SMOOTH_SNAP_DIST * SMOOTH_SNAP_DIST {
        return target;
    }
    let k = 1.0 - (-dt * core::f32::consts::LN_2 / SMOOTH_HALF_LIFE).exp();
    current.lerp(target, k.clamp(0.0, 1.0))
}

// ============================================================================
// Плагин
// ============================================================================

pub struct LyNetPlugin;

impl Plugin for LyNetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LyClient>()
            .init_resource::<HelloSent>()
            .init_resource::<PendingDiscrete>()
            .init_resource::<PrevHp>()
            // Карта для ПРЕДСКАЗАНИЯ движения: те же стены, что у сервера. Без неё
            // клиент предсказывал бы проход сквозь стены → откаты/дёрганье у стен.
            .init_resource::<MapGrids>();

        // Запись ввода в нативный ActionState (как в headless-примере).
        app.add_systems(
            FixedPreUpdate,
            buffer_input
                .in_set(InputSystems::WriteClientInputs)
                .run_if(in_state(AppState::InGame)),
        );
        // Предсказание движения локального игрока тем же шагом, что и сервер.
        app.add_systems(
            FixedUpdate,
            player_movement.run_if(in_state(AppState::InGame)),
        );

        app.add_observer(recv_fx);
        app.add_systems(
            Update,
            (send_hello, recv_auth, recv_scoreboard)
                .run_if(in_state(AppState::Connecting).or(in_state(AppState::InGame))),
        );

        app.add_systems(
            Update,
            (
                latch_discrete_input,
                attach_player_visuals,
                attach_npc_visuals,
                attach_grenade_visuals,
                sync_players,
                sync_npcs,
                sync_grenades,
                sync_local_status,
                sync_death_visibility,
                sync_hp_bars,
                emit_player_damage,
                update_dir_notch,
            )
                .chain()
                .after(InterpolationSystems::Interpolate)
                .before(crate::render::project_world_to_transform)
                .run_if(in_state(AppState::InGame)),
        );

        app.add_systems(OnExit(AppState::InGame), teardown_net);
    }
}

// ============================================================================
// Подключение
// ============================================================================

/// Случайный client_id netcode на процесс
fn random_client_id() -> u64 {
    (crate::platform::now_nanos() ^ crate::platform::random_u64().rotate_left(32)) | 1
}

/// Создаёт сущность-клиент и инициирует подключение к `server_addr`.
/// Возвращает её Entity (сохраняется в [`LyClient`]).
pub fn connect_to(
    commands: &mut Commands,
    server_addr: SocketAddr,
    cert_digest: &str,
) -> Entity {
    let client_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0);
    let auth = Authentication::Manual {
        server_addr,
        client_id: random_client_id(),
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            // Connect-токен штампуется ЧАСАМИ КЛИЕНТА, а сервер сверяет своими:
            // с дефолтными 30с любой сдвиг часов игрока > 30с приводил к тому,
            // что сервер МОЛЧА выбрасывал токен — «connection request timed out»
            // без единой ошибки (локально не воспроизводится: часы общие).
            // Час покрывает реальный разброс часов игроков; токен нужен только
            // на время хендшейка, так что риска тут нет.
            token_expire_secs: 3600,
            ..default()
        },
    )
    .expect("netcode client");
    // Нативно: balanced (0-3 тика задержки по RTT). В браузере добавляем ПОЛ
    // минимум 2 тика (30 мс): event loop браузера квантует отправку/приём по
    // кадрам rAF, и при около-нулевом RTT localhost инпуты без запаса приходят
    // на сервер ВПРИТЫК к своему тику (lightyear#1402) — любой хитч страницы
    // делает их опоздавшими (сервер шагает по прошлому вводу → откаты/дёрганье).
    #[cfg(not(target_arch = "wasm32"))]
    let input_delay = InputDelayConfig::balanced();
    #[cfg(target_arch = "wasm32")]
    let input_delay = InputDelayConfig {
        minimum_input_delay_ticks: 2,
        maximum_input_delay_before_prediction: 3,
        maximum_predicted_ticks: 7,
    };
    let input_timeline = InputTimelineConfig::default().with_input_delay(input_delay);
    let interpolation = InterpolationConfig::default()
        .with_min_delay(Duration::from_millis(10))
        .with_send_interval_ratio(1.7);

    #[cfg(not(target_arch = "wasm32"))]
    let io_bundle = {
        let _ = cert_digest;
        (UdpIo::default(),)
    };
    // Lightyear ждёт digest ЧИСТЫМ hex (без ':' из формата сервера/openssl).
    #[cfg(target_arch = "wasm32")]
    let io_bundle = (WebTransportClientIo {
        certificate_digest: cert_digest
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>()
            .to_lowercase(),
    },);

    let entity = commands
        .spawn((
            Name::from("LyClient"),
            Client::default(),
            Link::new(None),
            LocalAddr(client_addr),
            PeerAddr(server_addr),
            ReplicationReceiver::default(),
            PredictionManager::default(),
            input_timeline,
            interpolation,
            netcode,
            io_bundle,
        ))
        .id();
    commands.trigger(Connect { entity });
    entity
}

/// Полный сброс сетевой сессии (выход в меню / реконнект): отключаем клиента и
/// чистим все визуальные/реплицируемые сущности и связанные ресурсы.
fn teardown_net(
    mut commands: Commands,
    mut ly: ResMut<LyClient>,
    mut hello: ResMut<HelloSent>,
    mut prev: ResMut<PrevHp>,
    mut hp_ui: ResMut<HpUiMap>,
    mut npc_info: ResMut<NpcInfo>,
    mut npc_stun: ResMut<NpcStun>,
    clients: Query<Entity, With<Client>>,
    players: Query<Entity, With<Player>>,
    visuals: Query<Entity, Or<(With<PlayerMarker>, With<NpcMarker>, With<GrenadeViz>)>>,
) {
    for e in &clients {
        commands.entity(e).despawn();
    }
    for e in &players {
        commands.entity(e).despawn();
    }
    for e in &visuals {
        commands.entity(e).despawn();
    }
    for (_, e) in hp_ui.0.drain() {
        commands.entity(e).despawn();
    }
    ly.entity = None;
    hello.0 = false;
    prev.0.clear();
    npc_info.0.clear();
    npc_stun.0.clear();
}

// ============================================================================
// Авторизация / таблица очков
// ============================================================================

fn send_hello(
    mut hello: ResMut<HelloSent>,
    cfg: Res<ClientConfig>,
    mut sender: Query<&mut MessageSender<Hello>, With<Connected>>,
) {
    if hello.0 {
        return;
    }
    if let Ok(mut s) = sender.single_mut() {
        s.send::<AuthChannel>(Hello {
            name: cfg.name.clone(),
            password: cfg.password.clone(),
        });
        hello.0 = true;
        info!("[ly] sent Hello (name={})", cfg.name);
    }
}

fn recv_auth(
    mut ok: Query<&mut MessageReceiver<AuthOk>>,
    mut denied: Query<&mut MessageReceiver<AuthDenied>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut err: ResMut<crate::menu::ConnectError>,
) {
    for mut r in &mut ok {
        for _ in r.receive() {
            info!("[ly] AUTH OK");
            if matches!(state.get(), AppState::Connecting) {
                next.set(AppState::InGame);
            }
        }
    }
    for mut r in &mut denied {
        for m in r.receive() {
            warn!("[ly] AUTH DENIED: {}", m.reason);
            err.0 = Some(format!("Авторизация отклонена: {}", m.reason));
            next.set(AppState::Menu);
        }
    }
}

fn recv_scoreboard(
    mut sb: Query<&mut MessageReceiver<Scoreboard>>,
    mut data: ResMut<ScoreboardData>,
) {
    for mut r in &mut sb {
        for board in r.receive() {
            data.0 = board.0;
        }
    }
}

// ============================================================================
// Ввод + предсказание
// ============================================================================

/// Ловит разовые намерения (рывок/стан/бросок) в обычном `Update`, где доступны
/// курсор/камера, и складывает их в [`PendingDiscrete`] до записи `NetInput`.
/// Здесь же тикает клиентский КД гранаты (зеркало серверного): пока он не
/// прошёл, бросок не отправляется — полоска в UI честно показывает готовность.
fn latch_discrete_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingDiscrete>,
    mut nade_cd: ResMut<crate::resources::grenades::GrenadeCooldown>,
    windows: Query<&Window>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
) {
    nade_cd.0.tick(time.delta());
    if keys.just_pressed(KeyCode::Space) {
        pending.dash = true;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        pending.stun = true;
    }
    if keys.just_released(KeyCode::KeyG) && nade_cd.0.is_finished() {
        if let (Ok(window), Ok((cam, cam_tf)), Ok(_wp)) =
            (windows.single(), cam_q.single(), player_q.single())
        {
            if let Some(target) = crate::render::pointer_world(window, cam, cam_tf) {
                pending.throw = Some(target.trunc());
                nade_cd.0.reset();
            }
        }
    }
}

/// Записываем намерения игрока в `ActionState<NetInput>` управляемой сущности.
fn buffer_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    aim: Res<crate::resources::AimAngle>,
    windows: Query<&Window>,
    mut pending: ResMut<PendingDiscrete>,
    mut q: Query<&mut ActionState<NetInput>, With<InputMarker<NetInput>>>,
) {
    let Ok(mut action) = q.single_mut() else {
        return;
    };
    // Окно потеряло фокус (alt-tab/хитч): winit может «потерять» событие отпускания
    // клавиши, и игрок «убегает» с зажатой кнопкой. Пока окно не в фокусе — ввод
    // нулевой (стоим), чтобы не было неуправляемого бега по карте.
    if windows.iter().all(|w| !w.focused) {
        action.0 = NetInput {
            aim: aim.0,
            ..default()
        };
        *pending = PendingDiscrete::default();
        return;
    }
    // Изо-ремап WASD (экран → мир), как в прежнем send_input.
    let mut v = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        v += Vec2::new(1.0, 1.0);
    }
    if keys.pressed(KeyCode::KeyS) {
        v += Vec2::new(-1.0, -1.0);
    }
    if keys.pressed(KeyCode::KeyA) {
        v += Vec2::new(-1.0, 1.0);
    }
    if keys.pressed(KeyCode::KeyD) {
        v += Vec2::new(1.0, -1.0);
    }
    let inp = NetInput {
        up: v.y > 0.0,
        down: v.y < 0.0,
        left: v.x < 0.0,
        right: v.x > 0.0,
        aim: aim.0,
        block: mouse.pressed(MouseButton::Right),
        attack: mouse.pressed(MouseButton::Left),
        dash: pending.dash,
        stun: pending.stun,
        throw: pending.throw,
    };
    action.0 = inp;
    *pending = PendingDiscrete::default();
}

/// Двигаем предсказанного локального игрока тем же детерминированным шагом, что и
/// сервер (со скольжением вдоль стен из общей карты).
fn player_movement(
    mut q: Query<
        (
            &mut Position,
            &mut Rotation,
            &mut AbilityState,
            &mut Blocking,
            &ActionState<NetInput>,
            Option<&NetHealth>,
        ),
        With<Predicted>,
    >,
    map: Option<Res<MapGrids>>,
) {
    let cfg = server_cfg();
    let walls = map.as_deref().map(|m| &m.movement);
    for (mut pos, mut rot, mut abil, mut blk, action, hp) in &mut q {
        // мёртвый лежит и ждёт респауна — сервер ввод игнорирует, предсказывать
        // движение нечего (иначе получим дрейф и откат при каждом кадре)
        if hp.is_some_and(|h| h.0 <= 0) {
            continue;
        }
        step_player(&mut pos, &mut rot, &mut abil, &mut blk, &action.0, &cfg, walls);
    }
}

// ============================================================================
// Мост: навешивание визуала на реплицируемые сущности
// ============================================================================

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn attach_player_visuals(
    mut commands: Commands,
    knight: Option<Res<KnightAnims>>,
    ring_tex: Option<Res<crate::resources::RingTex>>,
    arrow_tex: Option<Res<crate::resources::ArrowTex>>,
    mut my: ResMut<MyPlayer>,
    mut hp_ui: ResMut<HpUiMap>,
    q: Query<
        (Entity, &Player, &Position, &Rotation, Has<Predicted>),
        (
            Or<(With<Predicted>, With<Interpolated>)>,
            Without<PlayerMarker>,
        ),
    >,
) {
    let (Some(knight), Some(ring_tex), Some(arrow_tex)) = (knight, ring_tex, arrow_tex) else {
        return;
    };
    for (entity, player, pos, rot, is_local) in &q {
        let id = player.0.to_bits();
        let world = pos.0;
        let rotation = rot.0;
        if is_local {
            my.id = id;
            my.got = true;
        }
        let ring_color = if is_local {
            Color::srgba(0.3, 1.0, 0.5, 0.95)
        } else {
            Color::srgba(1.0, 0.35, 0.35, 0.9)
        };
        commands
            .entity(entity)
            .insert((
                Sprite {
                    image: knight.idle.clone(),
                    texture_atlas: Some(TextureAtlas {
                        layout: knight.layout.clone(),
                        index: knight_row(rotation) * KNIGHT_COLS,
                    }),
                    custom_size: Some(Vec2::splat(ISO_ACTOR_PX)),
                    ..default()
                },
                bevy::sprite::Anchor(Vec2::new(0.0, KNIGHT_ANCHOR_Y)),
                WorldPos(world),
                RenderLayer(layers::ACTOR),
                PlayerMarker(id),
                Facing(rotation),
                ActorAnim {
                    prev: world,
                    ..default()
                },
                Name::new(format!(
                    "Player[{}] {id}",
                    if is_local { "LOCAL" } else { "REMOTE" }
                )),
            ))
            .with_children(|p| {
                p.spawn((
                    Sprite {
                        image: ring_tex.0.clone(),
                        color: ring_color,
                        custom_size: Some(Vec2::new(ISO_ACTOR_PX * 0.42, ISO_ACTOR_PX * 0.21)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, -0.5),
                ));
                p.spawn((
                    Sprite {
                        image: arrow_tex.0.clone(),
                        color: ring_color,
                        custom_size: Some(Vec2::splat(ISO_ACTOR_PX * 0.22)),
                        ..default()
                    },
                    dir_arrow_transform(rotation),
                    DirNotch,
                ));
            });
        if is_local {
            commands
                .entity(entity)
                .insert((LocalPlayer, InputMarker::<NetInput>::default()));
        }
        if !hp_ui.0.contains_key(&id) {
            let e = spawn_hp_ui(&mut commands, id, 100);
            hp_ui.0.insert(id, e);
        }
    }
}

#[allow(clippy::type_complexity)]
fn attach_npc_visuals(
    mut commands: Commands,
    anims: Option<Res<NpcAnims>>,
    q: Query<
        (
            Entity,
            Option<&Npc>,
            Option<&Confirmed<Npc>>,
            Option<&Position>,
            Option<&Confirmed<Position>>,
            Option<&Rotation>,
            Option<&Confirmed<Rotation>>,
        ),
        (
            Or<(With<Npc>, With<Confirmed<Npc>>)>,
            Without<NpcMarker>,
            Without<Player>,
        ),
    >,
) {
    let Some(anims) = anims else { return };
    for (entity, npc, cnpc, pos, cpos, rot, crot) in &q {
        let Some(kind) = read_comp(npc, cnpc).map(|n| n.kind) else {
            continue;
        };
        let Some(world) = read_comp(pos, cpos).map(|p| p.0) else {
            continue;
        };
        let facing = read_comp(rot, crot).map(|r| r.0).unwrap_or(0.0);
        let id = entity.to_bits() as u32;
        let (px, anchor_y) = npc_sprite_layout(kind);
        let vis = crate::systems::npc::npc_dir(facing);
        let screen = world_to_translation(world, layers::ACTOR);
        commands
            .entity(entity)
            .insert((
                Sprite {
                    image: anims.get(kind).walk[vis][0].clone(),
                    custom_size: Some(Vec2::splat(px)),
                    ..default()
                },
                bevy::sprite::Anchor(Vec2::new(0.0, anchor_y)),
                Transform::from_translation(screen),
                GlobalTransform::default(),
                Visibility::Visible,
                WorldPos(world),
                RenderLayer(layers::ACTOR),
                NpcMarker(id),
                NpcKindC(kind),
                Facing(facing),
                NpcAnim::default(),
                Name::new(format!("Npc {id} ({kind:?})")),
            ))
            .with_children(|p| {
                p.spawn((
                    Sprite {
                        color: Color::srgba(0.0, 0.0, 0.0, 0.65),
                        custom_size: Some(Vec2::new(42.0, 7.0)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 75.0, 0.05),
                ));
                p.spawn((
                    Sprite {
                        color: crate::systems::utils::hp_color(1.0),
                        custom_size: Some(Vec2::new(40.0, 5.0)),
                        ..default()
                    },
                    bevy::sprite::Anchor(Vec2::new(-0.5, 0.0)),
                    Transform::from_xyz(-20.0, 75.0, 0.06),
                    NpcHpFill { id, full_w: 40.0 },
                ));
            });
    }
}

#[allow(clippy::type_complexity)]
fn attach_grenade_visuals(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<
        (Entity, Option<&Position>, Option<&Confirmed<Position>>),
        (With<NetGrenade>, With<Interpolated>, Without<GrenadeViz>),
    >,
) {
    use crate::systems::grenade_lifecycle::make_screen_disc_mesh;
    use crate::systems::melee::make_iso_circle_mesh;
    for (entity, pos, cpos) in &q {
        let Some(world) = read_comp(pos, cpos).map(|p| p.0) else {
            continue;
        };
        let shadow_mesh = meshes.add(make_iso_circle_mesh(8.0, 20));
        let shadow_mat = materials.add(ColorMaterial::from(Color::srgba(0.0, 0.0, 0.0, 0.30)));
        let glow_mesh = meshes.add(make_screen_disc_mesh(11.0, 20));
        let glow_mat = materials.add(ColorMaterial::from(Color::srgba(0.55, 0.40, 1.0, 0.30)));
        let body_mesh = meshes.add(make_screen_disc_mesh(6.5, 20));
        let body_mat = materials.add(ColorMaterial::from(Color::srgba(0.78, 0.62, 1.0, 0.95)));
        commands
            .entity(entity)
            .insert((
                Mesh2d(shadow_mesh),
                MeshMaterial2d(shadow_mat),
                Transform::from_translation(world_to_translation(world, layers::EFFECT)),
                GlobalTransform::default(),
                Visibility::Visible,
                WorldPos(world),
                RenderLayer(layers::EFFECT),
                GrenadeViz,
                Name::new("Grenade"),
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh2d(glow_mesh),
                    MeshMaterial2d(glow_mat),
                    Transform::from_xyz(0.0, 12.0, 0.05),
                ));
                p.spawn((
                    Mesh2d(body_mesh),
                    MeshMaterial2d(body_mat),
                    Transform::from_xyz(0.0, 12.0, 0.10),
                ));
            });
    }
}

// ============================================================================
// Мост: синхронизация компонентов и анимаций каждый кадр
// ============================================================================

/// Копирует реплицируемые Position/Rotation в WorldPos/Facing игроков и заводит
/// одноразовые анимации удара/рывка/удара щитом по фронту кулдаунов (блок/стан —
/// напрямую). У локального игрока кулдауны ПРЕДСКАЗАННЫЕ, поэтому клип стартует
/// мгновенно и только когда способность реально сработала.
fn sync_players(
    mut prev: Local<HashMap<Entity, (f32, f32, f32)>>,
    time: Res<Time>,
    mut q: Query<(
        Entity,
        &Position,
        &Rotation,
        &AbilityState,
        &Blocking,
        &mut WorldPos,
        &mut Facing,
        &mut ActorAnim,
    )>,
) {
    let dt = time.delta_secs();
    for (e, pos, rot, abil, blk, mut wp, mut facing, mut anim) in &mut q {
        wp.0 = smooth_pos(wp.0, pos.0, dt);
        facing.0 = rot.0;
        let ab = &abil.0;
        anim.blocking = blk.0;
        anim.stun_left = ab.stun_left;
        let (pm, pd, ps) = prev.get(&e).copied().unwrap_or((0.0, 0.0, 0.0));
        // Каждый реальный удар (фронт КД) ОБЯЗАН показать взмах. Клип (0.625с)
        // чуть длиннее кулдауна (0.6с), поэтому перезапускаем даже недоигранный
        // Attack/Hurt — иначе каждый второй удар шёл бы без анимации («бьёт без
        // КД»). Не трогаем только Dash (во время рывка удара не бывает).
        if ab.melee_cd_left > pm + 0.01 && anim.state != AnimState::Dash {
            anim.start_action(AnimState::Attack);
        }
        if ab.dash_cd_left > pd + 0.01 {
            let ang = if ab.dash_dir.length_squared() > 1e-6 {
                ab.dash_dir.y.atan2(ab.dash_dir.x)
            } else {
                ab.facing
            };
            anim.start_action_facing(AnimState::Dash, ang);
        }
        // удар щитом (Q): Kick перебивает даже идущий одноразовый клип —
        // способность дискретная и редкая, её видимость важнее хвоста атаки.
        if ab.stun_cd_left > ps + 0.01 && anim.state != AnimState::Kick {
            anim.start_action(AnimState::Kick);
        }
        prev.insert(e, (ab.melee_cd_left, ab.dash_cd_left, ab.stun_cd_left));
    }
}

/// Копирует Position/Rotation неписей и публикует их видимое состояние в ресурсы,
/// которые читают существующие системы анимации/полосок/звука.
fn sync_npcs(
    time: Res<Time>,
    mut info: ResMut<NpcInfo>,
    mut stun: ResMut<NpcStun>,
    mut q: Query<
        (
            Entity,
            Option<&Position>,
            Option<&Confirmed<Position>>,
            Option<&Rotation>,
            Option<&Confirmed<Rotation>>,
            Option<&NetHealth>,
            Option<&Confirmed<NetHealth>>,
            Option<&NpcRuntime>,
            Option<&Confirmed<NpcRuntime>>,
            &mut WorldPos,
            &mut Facing,
        ),
        With<NpcMarker>,
    >,
) {
    let dt = time.delta_secs();
    info.0.clear();
    stun.0.clear();
    for (e, pos, cpos, rot, crot, hp, chp, rt, crt, mut wp, mut facing) in &mut q {
        let id = e.to_bits() as u32;
        let Some(world) = read_comp(pos, cpos).map(|p| p.0) else {
            continue;
        };
        wp.0 = smooth_pos(wp.0, world, dt);
        facing.0 = read_comp(rot, crot).map(|r| r.0).unwrap_or(facing.0);
        // HP/агро/атака — дискретное состояние: читаем СВЕЖЕЕ (Confirmed), не
        // интерполированное, иначе полоска HP отстаёт от факта попадания.
        let hp_val = read_fresh(hp, chp).map(|h| h.0).unwrap_or(0);
        let rt_val = read_fresh(rt, crt).unwrap_or(NpcRuntime {
            aggro: false,
            attacking: false,
            stun_left: 0.0,
        });
        info.0.insert(id, (hp_val, rt_val.aggro, rt_val.attacking));
        stun.0.insert(id, rt_val.stun_left);
    }
}

fn sync_grenades(
    mut q: Query<
        (
            Option<&Position>,
            Option<&Confirmed<Position>>,
            &mut WorldPos,
        ),
        With<GrenadeViz>,
    >,
) {
    for (pos, cpos, mut wp) in &mut q {
        if let Some(world) = read_comp(pos, cpos).map(|p| p.0) {
            wp.0 = world;
        }
    }
}

/// Питает UI/звук локального игрока (HP/стамина/блок/стан) из предсказанной
/// сущности и держит `LocalAbilities` для панели кулдаунов.
fn sync_local_status(
    q: Query<(&NetHealth, &AbilityState, &Blocking), (With<Player>, With<Predicted>)>,
    mut status: ResMut<LocalStatus>,
    mut abilities: ResMut<LocalAbilities>,
) {
    if let Ok((hp, abil, blk)) = q.single() {
        status.hp = hp.0;
        status.stamina = abil.0.stamina;
        status.blocking = blk.0;
        status.stun_left = abil.0.stun_left;
        abilities.0 = abil.0;
    }
}

/// Ведёт плавающие полоски HP игроков напрямую из СВЕЖЕГО реплицируемого HP.
/// Раньше ширина обновлялась только по событию урона (падению HP) — после
/// респауна (HP растёт 0 → 100) полоска оставалась пустой.
fn sync_hp_bars(
    q: Query<(
        &PlayerMarker,
        Option<&NetHealth>,
        Option<&Confirmed<NetHealth>>,
    )>,
    mut fills: Query<(&crate::components::HpFill, &mut Sprite)>,
) {
    use protocol::constants::PLAYER_MAX_HP;
    let mut hp_by_id: HashMap<u64, i32> = HashMap::new();
    for (m, hp, chp) in &q {
        if let Some(hp) = read_fresh(hp, chp) {
            hp_by_id.insert(m.0, hp.0);
        }
    }
    for (fill, mut sprite) in &mut fills {
        let Some(&hp) = hp_by_id.get(&fill.id) else {
            continue;
        };
        let frac = (hp as f32 / PLAYER_MAX_HP as f32).clamp(0.0, 1.0);
        sprite.color = crate::systems::utils::hp_color(frac);
        if let Some(sz) = sprite.custom_size.as_mut() {
            sz.x = fill.full_w * frac;
        }
    }
}

/// Прячет модель и HP-бар мёртвых игроков (HP ≤ 0): тело лежит трупом (его
/// рисует FX-труп со своей анимацией), а живая сущность ждёт респауна на
/// сервере — стоящий «призрак» на месте смерти выглядел бы как баг.
fn sync_death_visibility(
    hp_ui: Res<HpUiMap>,
    mut q: Query<(
        &PlayerMarker,
        Option<&NetHealth>,
        Option<&Confirmed<NetHealth>>,
        &mut Visibility,
    )>,
    mut vis_q: Query<&mut Visibility, Without<PlayerMarker>>,
) {
    for (m, hp, chp, mut vis) in &mut q {
        let alive = read_fresh(hp, chp).map(|h| h.0 > 0).unwrap_or(true);
        let want = if alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        if let Some(&ui) = hp_ui.0.get(&m.0) {
            if let Ok(mut v) = vis_q.get_mut(ui) {
                if *v != want {
                    *v = want;
                }
            }
        }
    }
}

/// Порождает события урона по падению реплицируемого HP игроков (вспышка/звук/
/// полоска): нового отдельного сообщения об уроне в протоколе нет. HP читаем
/// СВЕЖИМ (`Confirmed`), чтобы полоска/вспышка не отставали от попадания.
fn emit_player_damage(
    mut prev: ResMut<PrevHp>,
    mut ev: MessageWriter<PlayerDamagedEvent>,
    q: Query<(
        &PlayerMarker,
        Option<&NetHealth>,
        Option<&Confirmed<NetHealth>>,
    )>,
) {
    let mut alive: Vec<u64> = Vec::new();
    for (m, hp, chp) in &q {
        let Some(hp) = read_fresh(hp, chp).map(|h| h.0) else {
            continue;
        };
        alive.push(m.0);
        let old = prev.0.get(&m.0).copied().unwrap_or(hp);
        if hp < old {
            ev.write(PlayerDamagedEvent {
                id: m.0,
                new_hp: hp,
                damage: old - hp,
            });
        }
        prev.0.insert(m.0, hp);
    }
    prev.0.retain(|id, _| alive.contains(id));
}

/// Локальный (экранный) трансформ стрелки-указателя направления для мирового
/// угла `facing`: позиция чуть за ободком-эллипсом + доворот в ИЗО-проекции.
pub fn dir_arrow_transform(facing: f32) -> Transform {
    use crate::render::world_to_screen;
    let rw = ISO_ACTOR_PX * 0.21 / std::f32::consts::SQRT_2;
    let s = world_to_screen(Vec2::new(facing.cos(), facing.sin())) * rw;
    let angle = s.y.atan2(s.x);
    Transform {
        translation: Vec3::new(s.x * 1.18, s.y * 1.18, -0.45),
        rotation: Quat::from_rotation_z(angle),
        ..default()
    }
}

/// Каждый кадр ведёт стрелку-указатель по ободку в направлении взгляда (`Facing`).
fn update_dir_notch(
    q_face: Query<&Facing>,
    mut q_notch: Query<(&ChildOf, &mut Transform), With<DirNotch>>,
) {
    for (parent, mut tf) in &mut q_notch {
        if let Ok(facing) = q_face.get(parent.parent()) {
            *tf = dir_arrow_transform(facing.0);
        }
    }
}

// ============================================================================
// Трансляция FX-событий (Lightyear RemoteEvent) в клиентские события/визуал
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn recv_fx(
    trigger: On<RemoteEvent<Fx>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    knight: Option<Res<KnightAnims>>,
    npc_anims: Option<Res<NpcAnims>>,
    mut corpses: ResMut<Corpses>,
    q_npcs: Query<(Entity, &WorldPos, &Facing), With<NpcMarker>>,
    mut ev_combat: MessageWriter<CombatSfxEvent>,
    mut ev_npc_died: MessageWriter<NpcDiedEvent>,
    mut ev_npc_sound: MessageWriter<NpcSoundEvent>,
    mut ev_detonated: MessageWriter<GrenadeDetonatedEvent>,
) {
    match trigger.event().trigger {
        Fx::Combat { kind, pos, dir } => match kind {
            FxKind::Melee => {
                crate::systems::melee::spawn_player_melee_decal(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    pos,
                    dir,
                );
                ev_combat.write(CombatSfxEvent {
                    kind: CombatSfxKind::Swing,
                    pos,
                });
            }
            FxKind::Stun => {
                ev_combat.write(CombatSfxEvent {
                    kind: CombatSfxKind::StunBash,
                    pos,
                });
            }
            FxKind::Dash => {
                ev_combat.write(CombatSfxEvent {
                    kind: CombatSfxKind::Dash,
                    pos,
                });
            }
            FxKind::Blocked => {
                ev_combat.write(CombatSfxEvent {
                    kind: CombatSfxKind::Blocked,
                    pos,
                });
            }
        },
        Fx::Detonation { pos } => {
            ev_detonated.write(GrenadeDetonatedEvent { id: 0, pos });
        }
        Fx::Death { kind, pos } => match kind {
            DeathKind::Player => {
                if let Some(knight) = knight.as_deref() {
                    let row = knight_row(0.0);
                    let corpse = commands
                        .spawn((
                            Sprite {
                                image: knight.death.clone(),
                                texture_atlas: Some(TextureAtlas {
                                    layout: knight.layout.clone(),
                                    index: row * KNIGHT_COLS,
                                }),
                                custom_size: Some(Vec2::splat(ISO_ACTOR_PX)),
                                ..default()
                            },
                            bevy::sprite::Anchor(Vec2::new(0.0, KNIGHT_ANCHOR_Y)),
                            Transform::from_translation(world_to_translation(
                                pos,
                                layers::CORPSE,
                            )),
                            GlobalTransform::default(),
                            WorldPos(pos),
                            RenderLayer(layers::CORPSE),
                            Corpse {
                                row,
                                frame: 0,
                                anim: Timer::from_seconds(1.0 / 24.0, TimerMode::Repeating),
                            },
                        ))
                        .id();
                    corpses.register(&mut commands, corpse);
                }
            }
            DeathKind::Npc(npc_kind) => {
                ev_npc_died.write(NpcDiedEvent { pos });
                // Живую сущность деспавнит репликация (сервер убил её в тот же
                // тик); анимацию смерти играет труп с 0-го кадра. Направление
                // берём с ещё живой модели, если она рядом.
                let facing = nearest_npc(pos, &q_npcs).map(|(_, f)| f).unwrap_or(0.0);
                if let Some(anims) = npc_anims.as_deref() {
                    let corpse = spawn_npc_corpse(&mut commands, anims, npc_kind, pos, facing);
                    corpses.register(&mut commands, corpse);
                }
            }
        },
        Fx::NpcSound { kind, pos } => {
            ev_npc_sound.write(NpcSoundEvent { kind, pos });
        }
    }
}
