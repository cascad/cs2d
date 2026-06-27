mod components;
mod config;
mod console;
mod constants;
mod events;
mod render;
mod resources;
mod systems;
mod ui;

// +++ добавили +++
mod app_state;
mod lobby;
mod menu;
mod pause_menu;

use std::collections::VecDeque;

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClientPlugin;

use crate::console::{capture_console_layer, setup_console_ui, toggle_console, update_console_ui};

use protocol::constants::TICK_DT;
use protocol::messages::Stance;

use resources::*;
use systems::{
    bullet_lifecycle::bullet_lifecycle,
    connection::handle_connection_event,
    grenade_lifecycle::explosion_lifecycle, // grenade_lifecycle::grenade_lifecycle,
    grenade_throw::grenade_throw,
    input::change_stance,
    interpolate_with_snapshot::interpolate_with_snapshot,
    melee::{melee_arc_lifecycle, melee_attack},
    network::receive_server_messages,
    ping::send_ping,
    rotate_to_cursor::rotate_to_cursor,
    send_input::{send_input_and_predict, smooth_local_player},
    shoot::shoot_mouse,
    startup::setup,
};
use ui::update_grenade_cooldown_ui::update_grenade_cooldown_ui;

use crate::{
    app_state::AppState,
    events::{
        GrenadeDetonatedEvent, GrenadeSpawnEvent, PlayerDamagedEvent, PlayerDied, PlayerLeftEvent,
    },
    menu::{clear_connect_timeout, connection_timeout_system, MenuPlugin},
    resources::grenades::{ClientGrenades, GrenadeCooldown, GrenadeStates},
    systems::{
        // +++ насос Connecting: ждём первый Snapshot, затем -> InGame +++
        aim::{spawn_aim_marker, update_aim_to_mouse}, camera::CameraFollowPlugin, connecting_pump::connecting_pump, corpse_lc::corpse_lifecycle, ensure_my_id::ensure_my_id_from_conn, fog::{fade_unseen_players, setup_fog, update_fog}, grenade_lifecycle::spawn_grenades, iso::{animate_actors, setup_iso}, level_fixed::setup_fixed_level, network::apply_grenade_net, render_detonations::render_detonations, spawn_damage_popups::{spawn_damage_popups, update_damage_popups}, startup::load_ui_font, sync_hp_ui::{
            cleanup_hp_ui_on_player_remove, sync_hp_ui_position, update_hp_text_from_event,
        }, walls_cache::build_wall_aabb_cache
    },
    ui::cooldowns_ui::{setup_cooldowns_ui, update_cooldowns_ui},
    ui::grenade_ui::setup_grenade_ui,
    ui::hp_hud::{setup_hp_hud, update_hp_hud},
    ui::minimap::{setup_minimap, toggle_minimap, update_minimap},
    ui::stamina_ui::{setup_stamina_ui, update_stamina_ui},
};

/// Где искать ассеты. Один и тот же бинарь должен работать и из исходников
/// (`cargo run`), и из распакованного релиза (рядом лежит папка `assets`).
/// Возвращаем АБСОЛЮТНЫЙ путь — Bevy подставит его как корень источника ассетов.
fn asset_root() -> String {
    use std::path::PathBuf;
    // 1) рядом с исполняемым файлом: <каталог exe>/assets (распакованный zip)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("assets");
            if p.is_dir() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    // 2) запуск из исходников: <crate>/../assets = assets в корне репозитория.
    //    Канонизируем, чтобы убрать `..` из середины пути (надёжнее для ридера).
    let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    if dev.is_dir() {
        let clean = std::fs::canonicalize(&dev).unwrap_or(dev);
        return clean.to_string_lossy().into_owned();
    }
    // 3) запасной вариант — относительный путь от рабочего каталога
    "assets".to_string()
}

fn main() {
    App::new()
        // ресурсы
        // конфиг клиента (ip/port сервера) из файла рядом с бинарём
        .insert_resource(crate::config::load_or_create())
        .insert_resource(MyPlayer { id: 0, got: false })
        .insert_resource(TimeSync { offset: 0.0 })
        .insert_resource(SnapshotBuffer {
            snapshots: VecDeque::new(),
            delay: 0.05,
        })
        .insert_resource(CurrentStance(Stance::Standing))
        .insert_resource(crate::resources::AimAngle::default())
        .insert_resource(SendTimer(Timer::from_seconds(
            TICK_DT,
            TimerMode::Repeating,
        )))
        .insert_resource(SpawnedPlayers::default())
        .insert_resource(SeqCounter(0))
        .insert_resource(PendingInputsClient::default())
        .insert_resource(HeartbeatTimer::default())
        .insert_resource(ClientLatency::default())
        .insert_resource(DeadPlayers::default())
        .insert_resource(GrenadeCooldown::default())
        .insert_resource(HpUiMap::default())
        .insert_resource(SolidTiles::default())
        .insert_resource(ClientGrenades::default())
        .insert_resource(GrenadeStates::default())
        .insert_resource(WallAabbCache::default())
        .insert_resource(WallGridRes::default())
        .insert_resource(LastKnownPos::default())
        .insert_resource(LastSeen::default())
        .insert_resource(LocalAbilities::default())
        .insert_resource(LocalStatus::default())
        .insert_resource(PredictedPos::default())
        .insert_resource(crate::resources::Corpses::default())
        .insert_resource(crate::systems::npc::SpawnedNpcs::default())
        .insert_resource(crate::systems::npc::NpcInfo::default())
        .insert_resource(crate::systems::npc::NpcLastSeen::default())
        // ивенты
        .add_message::<PlayerDamagedEvent>()
        .add_message::<PlayerDied>()
        .add_message::<PlayerLeftEvent>()
        .add_message::<GrenadeSpawnEvent>()
        .add_message::<GrenadeDetonatedEvent>()
        // плагины
        .add_plugins(
            DefaultPlugins
                // корень ассетов: рядом с бинарём (релиз) или в корне репо (dev)
                .set(AssetPlugin {
                    file_path: asset_root(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "CS-style Multiplayer Client".into(),
                        resolution: (1024, 768).into(),
                        ..default()
                    }),
                    ..default()
                })
                // Захват логов в внутриигровую консоль (тоггл по `~`).
                .set(LogPlugin {
                    level: Level::INFO,
                    filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_winit=warn".into(),
                    custom_layer: capture_console_layer,
                    ..default()
                }),
        )
        .add_plugins(QuinnetClientPlugin::default())
        .add_plugins(CameraFollowPlugin)
        // (опционально) сразу включить плавный режим:
        // .insert_resource(CameraFollowSettings { mode: FollowMode::Smooth, ..default() })
        // состояния и меню
        .insert_state(AppState::Menu)
        .add_plugins(MenuPlugin)
        .add_plugins(crate::pause_menu::PauseMenuPlugin)
        // --- шрифты грузим заранее (нужны в меню тоже) ---
        .add_systems(Startup, (load_ui_font, setup_console_ui))
        // --- консоль логов работает в любом состоянии (тоггл по `~`) ---
        .add_systems(Update, (toggle_console, update_console_ui))
        // --- Connecting: ждём первый снапшот и следим за таймаутом ---
        .add_systems(
            Update,
            (connecting_pump, connection_timeout_system).run_if(in_state(AppState::Connecting)),
        )
        // сброс таймера при входе в игру
        .add_systems(OnEnter(AppState::InGame), clear_connect_timeout)
        // --- загрузка уровня и UI при входе в InGame ---
        .add_systems(
            OnEnter(AppState::InGame),
            (
                setup,
                setup_fixed_level,
                setup_iso,
                crate::systems::npc::setup_npc_anims,
                setup_grenade_ui,
                setup_stamina_ui,
                setup_cooldowns_ui,
                setup_hp_hud,
                setup_fog,
                setup_minimap,
            ),
        )
        // --- PreUpdate: сетка/инпут и приём сообщений только в InGame ---
        .add_systems(
            PreUpdate,
            (send_input_and_predict, handle_connection_event)
                .chain()
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            PreUpdate,
            (ensure_my_id_from_conn, receive_server_messages)
                .chain()
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(OnEnter(AppState::InGame), spawn_aim_marker)
        .add_systems(Update, update_aim_to_mouse.run_if(in_state(AppState::InGame)))
        // --- Update: вся игровая логика только в InGame ---
        .add_systems(
            Update,
            (
                smooth_local_player,
                interpolate_with_snapshot,
                bullet_lifecycle,
                // grenades
                spawn_grenades,
                apply_grenade_net,
                render_detonations,
                //
                explosion_lifecycle,
                grenade_throw,
                rotate_to_cursor,
                change_stance,
                shoot_mouse,
                melee_attack,
                melee_arc_lifecycle,
                send_ping,
                // туман: перестраиваем затемнение от позиции игрока (после движения)
                update_fog,
                // анимация направленных спрайтов: ПОСЛЕ движения WorldPos
                animate_actors,
                // засечка направления на кольце: ПОСЛЕ обновления Facing
                crate::systems::network::update_dir_notch,
                // НЕПИСИ: спавн/интерполяция/анимация скелетов и трупов
                (
                    crate::systems::npc::apply_npc_snapshot,
                    crate::systems::npc::interpolate_npcs,
                    crate::systems::npc::animate_skeletons,
                    crate::systems::npc::animate_skeleton_corpses,
                    crate::systems::npc::spawn_npc_attack_decals,
                )
                    .chain(),
                // проекция мир→экран: ПОСЛЕ всех, кто двигает WorldPos, и ДО камеры
                crate::render::project_world_to_transform,
            )
                .chain()
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            Update,
            (
                update_grenade_cooldown_ui,
                update_stamina_ui,
                update_cooldowns_ui,
                update_hp_hud,
                spawn_damage_popups,
                update_damage_popups,
                crate::systems::iso::flash_on_damage,
                sync_hp_ui_position,
                update_hp_text_from_event,
                cleanup_hp_ui_on_player_remove,
                corpse_lifecycle,
                fade_unseen_players,
                crate::systems::npc::update_npc_hp_bars,
                crate::systems::npc::fade_unseen_npcs,
                update_minimap,
                toggle_minimap,
            )
                .run_if(in_state(AppState::InGame)),
        )
        // --- PostUpdate: кэш стен и финализация цветов тоже только в InGame ---
        .add_systems(
            PostUpdate,
            (
                build_wall_aabb_cache,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .run();
}
