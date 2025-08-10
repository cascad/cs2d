mod components;
mod constants;
mod events;
mod resources;
mod systems;
mod ui;

// +++ добавили +++
mod app_state;
mod menu;

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClientPlugin;

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
    network::receive_server_messages,
    ping::send_ping,
    rotate_to_cursor::rotate_to_cursor,
    send_input::send_input_and_predict,
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
        connecting_pump::connecting_pump,
        corpse_lc::corpse_lifecycle,
        ensure_my_id::ensure_my_id_from_conn,
        camera::{CameraFollowPlugin},
        grenade_lifecycle::spawn_grenades,
        level::fill_solid_tiles_once,
        level_fixed::setup_fixed_level,
        network::apply_grenade_net,
        render_detonations::render_detonations,
        spawn_damage_popups::{spawn_damage_popups, update_damage_popups},
        startup::load_ui_font,
        sync_hp_ui::{
            cleanup_hp_ui_on_player_remove, sync_hp_ui_position, update_hp_text_from_event,
        },
        walls_cache::build_wall_aabb_cache,
    },
    ui::grenade_ui::setup_grenade_ui,
};

fn main() {
    App::new()
        // ресурсы
        .insert_resource(MyPlayer { id: 0, got: false })
        .insert_resource(TimeSync { offset: 0.0 })
        .insert_resource(SnapshotBuffer {
            snapshots: VecDeque::new(),
            delay: 0.05,
        })
        .insert_resource(CurrentStance(Stance::Standing))
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
        .insert_resource(LastKnownPos::default())
        // ивенты
        .add_event::<PlayerDamagedEvent>()
        .add_event::<PlayerDied>()
        .add_event::<PlayerLeftEvent>()
        .add_event::<GrenadeSpawnEvent>()
        .add_event::<GrenadeDetonatedEvent>()
        // плагины
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CS-style Multiplayer Client".into(),
                resolution: (1024.0, 768.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(QuinnetClientPlugin::default())
        .add_plugins(CameraFollowPlugin)
        // (опционально) сразу включить плавный режим:
        // .insert_resource(CameraFollowSettings { mode: FollowMode::Smooth, ..default() })
        // состояния и меню
        .insert_state(AppState::Menu)
        .add_plugins(MenuPlugin)
        // --- шрифты грузим заранее (нужны в меню тоже) ---
        .add_systems(Startup, load_ui_font)
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
                setup_grenade_ui,
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
        // --- Update: вся игровая логика только в InGame ---
        .add_systems(
            Update,
            (
                fill_solid_tiles_once,
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
                send_ping,
            )
                .chain()
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            Update,
            (
                update_grenade_cooldown_ui,
                spawn_damage_popups,
                update_damage_popups,
                sync_hp_ui_position,
                update_hp_text_from_event,
                cleanup_hp_ui_on_player_remove,
                corpse_lifecycle,
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
