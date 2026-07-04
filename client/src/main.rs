mod components;
mod config;
mod console;
mod constants;
mod events;
mod platform;
mod render;
mod resources;
mod systems;
mod ui;

mod app_state;
#[cfg(not(target_arch = "wasm32"))]
mod lobby;
mod lynet;
mod menu;
mod pause_menu;
#[cfg(target_arch = "wasm32")]
mod wasm_boot;
mod wasm_boot_plugin;

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use lightyear::prelude::client::ClientPlugins;

use crate::console::{capture_console_layer, setup_console_ui, toggle_console, update_console_ui};

use resources::*;
use systems::{
    grenade_lifecycle::explosion_lifecycle,
    grenade_throw::{setup_grenade_aim, update_grenade_aim},
    melee::{melee_arc_lifecycle, setup_melee_hint, update_melee_hint},
    rotate_to_cursor::rotate_to_cursor,
    startup::{despawn_game_camera, setup},
};
use ui::update_grenade_cooldown_ui::update_grenade_cooldown_ui;

use crate::{
    app_state::AppState,
    events::{GrenadeDetonatedEvent, PlayerDamagedEvent},
    lynet::LyNetPlugin,
    menu::{clear_connect_timeout, connection_timeout_system, MenuPlugin},
    resources::grenades::{ClientGrenades, GrenadeCooldown, GrenadeStates},
    systems::{
        aim::{spawn_aim_marker, update_aim_to_mouse},
        camera::CameraFollowPlugin,
        corpse_lc::corpse_lifecycle,
        fog::{apply_fog_tint, setup_fog, update_fog},
        iso::{animate_actors, setup_iso},
        level_fixed::setup_fixed_level,
        render_detonations::{animate_explosion_fx, render_detonations},
        spawn_damage_popups::{spawn_damage_popups, update_damage_popups},
        startup::load_ui_font,
        sync_hp_ui::{cleanup_hp_ui_on_player_remove, sync_hp_ui_position},
        walls_cache::build_wall_aabb_cache,
    },
    ui::cooldowns_ui::{setup_cooldowns_ui, update_cooldowns_ui},
    ui::damage_flash::{setup_damage_flash, update_damage_flash},
    ui::grenade_ui::setup_grenade_ui,
    ui::hp_hud::{setup_hp_hud, update_hp_hud},
    ui::minimap::{setup_minimap, toggle_minimap, update_minimap},
    ui::scoreboard::{setup_scoreboard_ui, update_scoreboard_ui},
    ui::stamina_ui::{setup_stamina_ui, update_stamina_ui},
    ui::status_effects::{setup_status_effects_ui, update_status_effects_ui},
};

/// Где искать ассеты. Native: рядом с exe или `../assets`. Wasm: HTTP-путь `assets/`.
fn asset_root() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        return "assets".to_string();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::path::PathBuf;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let p = dir.join("assets");
                if p.is_dir() {
                    return p.to_string_lossy().into_owned();
                }
            }
        }
        let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
        if dev.is_dir() {
            let clean = std::fs::canonicalize(&dev).unwrap_or(dev);
            return clean.to_string_lossy().into_owned();
        }
        "assets".to_string()
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    App::new()
        // ресурсы
        // конфиг клиента (ip/port сервера) из файла рядом с бинарём
        .insert_resource(crate::config::load_or_create())
        .insert_resource(MyPlayer { id: 0, got: false })
        .insert_resource(crate::resources::AimAngle::default())
        .insert_resource(GrenadeCooldown::default())
        .insert_resource(HpUiMap::default())
        .insert_resource(SolidTiles::default())
        .insert_resource(ClientGrenades::default())
        .insert_resource(GrenadeStates::default())
        .insert_resource(WallAabbCache::default())
        .insert_resource(WallGridRes::default())
        .insert_resource(VisionGridRes::default())
        .insert_resource(LocalAbilities::default())
        .insert_resource(LocalStatus::default())
        .insert_resource(crate::resources::Corpses::default())
        .insert_resource(crate::resources::ScoreboardData::default())
        .insert_resource(crate::systems::npc::NpcInfo::default())
        .insert_resource(crate::systems::npc::NpcStun::default())
        .add_message::<PlayerDamagedEvent>()
        .add_message::<GrenadeDetonatedEvent>()
        .add_message::<crate::events::NpcDiedEvent>()
        .add_message::<crate::events::NpcSoundEvent>()
        .add_message::<crate::events::CombatSfxEvent>()
        // звук: пулы клипов + ГСЧ выбора (амбиентные рыки теперь шлёт сервер)
        .init_resource::<crate::systems::audio::AudioRng>()
        .init_resource::<crate::systems::audio::BgMusicIdx>()
        // плагины
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root(),
                    #[cfg(target_arch = "wasm32")]
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "CS2D".into(),
                        resolution: (1024, 768).into(),
                        // По центру основного монитора (по умолчанию ОС кидает в угол).
                        // На wasm игнорируется (там канвас, а не окно).
                        position: WindowPosition::Centered(MonitorSelection::Primary),
                        #[cfg(target_arch = "wasm32")]
                        prevent_default_event_handling: true,
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
        // Фон «пустоты» за пределами комнат: почти чёрный (тьма подземелья).
        // Дефолтный серо-синий ClearColor ломал атмосферу вокруг карты.
        .insert_resource(ClearColor(Color::srgb(0.045, 0.04, 0.055)))
        // --- Lightyear: сетевой движок + протокол + наш мост к визуалу ---
        .add_plugins(ClientPlugins {
            tick_duration: netproto::tick_duration(),
        })
        .add_plugins(netproto::ProtocolPlugin)
        .add_plugins(LyNetPlugin)
        .add_plugins(CameraFollowPlugin)
        // (опционально) сразу включить плавный режим:
        // .insert_resource(CameraFollowSettings { mode: FollowMode::Smooth, ..default() })
        // состояния и меню
        .insert_state(AppState::Menu)
        .add_plugins(MenuPlugin)
        .add_plugins(crate::pause_menu::PauseMenuPlugin)
        .add_plugins(crate::wasm_boot_plugin::WasmBootPlugin)
        // --- шрифты грузим заранее (нужны в меню тоже) ---
        .add_systems(Startup, (load_ui_font, setup_console_ui))
        // --- звук: грузим клипы на старте; музыка вкл/выкл по входу/выходу с карты ---
        .add_systems(Startup, crate::systems::audio::setup_audio)
        .add_systems(OnEnter(AppState::InGame), crate::systems::audio::start_bg_music)
        .add_systems(OnExit(AppState::InGame), crate::systems::audio::stop_bg_music)
        .add_systems(
            Update,
            crate::systems::audio::advance_bg_music.run_if(in_state(AppState::InGame)),
        )
        // --- консоль логов работает в любом состоянии (тоггл по `~`) ---
        .add_systems(Update, (toggle_console, update_console_ui))
        // --- Connecting: ждём авторизацию и следим за таймаутом ---
        .add_systems(
            Update,
            connection_timeout_system.run_if(in_state(AppState::Connecting)),
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
                setup_damage_flash,
                setup_fog,
                setup_minimap,
                setup_melee_hint,
                setup_grenade_aim,
                setup_scoreboard_ui,
                setup_status_effects_ui,
            ),
        )
        .add_systems(OnEnter(AppState::InGame), spawn_aim_marker)
        // выход из игры (возврат в меню/реконнект): убираем игровую камеру, чтобы
        // не копились камеры с одинаковым order (рендер иначе спамит warning)
        .add_systems(OnExit(AppState::InGame), despawn_game_camera)
        .add_systems(Update, update_aim_to_mouse.run_if(in_state(AppState::InGame)))
        // --- звук по событиям боя (только на карте) ---
        .add_systems(
            Update,
            (
                crate::systems::audio::play_player_hit_sfx,
                crate::systems::audio::play_npc_hit_sfx,
                crate::systems::audio::play_npc_death_sfx,
                crate::systems::audio::play_npc_sound_sfx,
                crate::systems::audio::play_combat_sfx,
                crate::systems::audio::play_explosion_sfx,
            )
                .run_if(in_state(AppState::InGame)),
        )
        // --- Update: вся игровая логика только в InGame ---
        .add_systems(
            Update,
            (
                render_detonations,
                animate_explosion_fx,
                explosion_lifecycle,
                rotate_to_cursor,
                melee_arc_lifecycle,
                // туман: пересчёт сетки видимости (после движения) и сразу тинт
                // спрайтов карты по ней — затемнение видно прямо на полу/стенах.
                update_fog,
                apply_fog_tint,
                // анимация направленных спрайтов: ПОСЛЕ движения WorldPos
                animate_actors,
                // НЕПИСИ: анимация скелетов/зомби и трупов (состояние из реплики)
                (
                    crate::systems::npc::apply_npc_sound_anims,
                    crate::systems::npc::animate_skeletons,
                    crate::systems::npc::animate_skeleton_corpses,
                    crate::systems::npc::spawn_npc_attack_decals,
                    // звёздочки оглушения над игроками/неписями: ПОСЛЕ стана/спавна
                    crate::systems::stun_stars::update_stun_stars,
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
                update_damage_flash,
                update_melee_hint,
                update_grenade_aim,
                spawn_damage_popups,
                update_damage_popups,
                crate::systems::iso::flash_on_damage,
                sync_hp_ui_position,
                cleanup_hp_ui_on_player_remove,
                corpse_lifecycle,
                crate::systems::npc::update_npc_hp_bars,
                update_minimap,
                toggle_minimap,
                (update_scoreboard_ui, update_status_effects_ui),
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
