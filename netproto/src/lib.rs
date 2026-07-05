//! Общий сетевой слой на Lightyear 0.26.
//!
//! Здесь живут: протокол (реплицируемые компоненты, инпуты, каналы, сообщения) и
//! детерминированная симуляция, общая для клиента (предсказание) и сервера
//! (авторитет). Это ядро миграции с самописного стека (bevy_quinnet + ручные
//! снапшоты) на Lightyear.
//!
//! Stage 1: полный протокол (компоненты + нативный инпут + сообщения авторизации
//! и таблицы очков) регистрируется в `ProtocolPlugin`. Предсказание/интерполяция
//! по компонентам и каналы настроим в следующих стадиях.

pub mod auth;
pub mod components;
pub mod fx;
pub mod grenade;
pub mod input;
pub mod interest;
pub mod map;
pub mod messages;
pub mod npc;
pub mod server_sim;
pub mod sim;

pub use auth::{
    Accounts, AccountsFile, AuthChannel, Authed, ScoreboardDirty, ServerLimits,
    broadcast_scoreboard, kick_afk_players, load_accounts, on_disconnect_cleanup,
    server_handle_auth, spawn_authed_player,
};
pub use components::*;
pub use fx::{FxChannel, FxOut, flush_fx};
pub use grenade::{Grenade, GrenadePhysics, spawn_grenade, spawn_grenades, update_grenades};
pub use input::NetInput;
pub use interest::update_interest;
pub use map::{MapGrids, TILE};
pub use messages::*;
pub use npc::{NpcBrain, NpcGrowls, NpcRespawns, npc_ai, npc_growls, respawn_npcs, spawn_npc};
pub use server_sim::{
    DamageAttribution, Dead, PendingStrikes, Strike, Strikes, apply_player_inputs,
    resolve_player_deaths, respawn_players, server_cfg, server_simulate,
};
pub use sim::{slide, step_player};

use core::time::Duration;

/// Длительность одного тика симуляции (= `protocol::constants::TICK_DT`). Должна
/// совпадать на клиенте и сервере: её передаём в `Client/ServerPlugins`.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(protocol::constants::TICK_DT as f64)
}

/// UDP-порт сервера по умолчанию.
pub const SERVER_PORT: u16 = 5000;

/// Идентификатор протокола netcode: клиент и сервер должны совпадать, иначе
/// рукопожатие отклоняется. Меняем при несовместимых изменениях протокола.
pub const PROTOCOL_ID: u64 = 0xC520_2D01;

/// Приватный ключ netcode (32 байта). Пока нулевой (небезопасно, но достаточно
/// для локальной разработки); позже вынесем в env `LIGHTYEAR_PRIVATE_KEY`.
pub const PRIVATE_KEY: [u8; 32] = [0u8; 32];

use bevy::prelude::{App, Plugin};
use lightyear::prelude::input::native::InputPlugin as NativeInputPlugin;
use lightyear::prelude::*;

/// Функция «интерполяции» без сглаживания: для дискретных компонентов (тип неписи,
/// HP, способности, блок, маркеры) нам нужна не плавность, а сам факт доставки
/// значения на интерполируемую сущность. Возвращаем целевое значение как есть.
fn snap<C: Clone>(_start: C, end: C, _t: f32) -> C {
    end
}

/// Откатывать предсказание позиции только при ЗАМЕТНОМ расхождении (> ~1 ед.).
/// Микроразницы из-за порядка float-операций/тайминга не должны вызывать откат —
/// иначе локальный игрок «дрожит» на месте/в движении. Сервер всё равно остаётся
/// авторитетом: реальное расхождение (> порога) скорректируется.
fn pos_should_rollback(confirmed: &Position, predicted: &Position) -> bool {
    confirmed.0.distance_squared(predicted.0) > 1.0
}

/// Поворот: откат только при ЗАМЕТНОЙ разнице (~3°). Прицел (aim) меняется
/// каждый тик движения мыши, и на реальном RTT сервер закономерно применяет его
/// на ±1 тик позже клиента — разница составляет до turn_rate·dt ≈ 0.21 рад.
/// Прежний порог 0.01 рад (0.6°) превращал КАЖДЫЙ пакет в откат — визуальный
/// поворот «мылился» коррекцией и казался страшно неотзывчивым (в браузере на
/// проде особенно). 3° глазом неразличимы, сервер остаётся авторитетом.
fn rot_should_rollback(confirmed: &Rotation, predicted: &Rotation) -> bool {
    use core::f32::consts::{PI, TAU};
    ((confirmed.0 - predicted.0 + PI).rem_euclid(TAU) - PI).abs() > 0.05
}

/// AbilityState сравнивался ПОБАЙТОВО (PartialEq): любое f32-расхождение из-за
/// сдвига применения ввода на ±1 тик (facing/стамина/КД) вызывало откат каждый
/// пакет — вместе с жёстким порогом Rotation это и давало «ватный» поворот.
/// Сравниваем с допусками: реальная рассинхронизация (сработала способность,
/// большой дрейф) откатится, микродрейф — нет.
fn abil_should_rollback(confirmed: &AbilityState, predicted: &AbilityState) -> bool {
    use core::f32::consts::{PI, TAU};
    let (c, p) = (&confirmed.0, &predicted.0);
    let ang = |a: f32, b: f32| ((a - b + PI).rem_euclid(TAU) - PI).abs();
    ang(c.facing, p.facing) > 0.05
        || (c.stamina - p.stamina).abs() > 2.0
        || (c.dash_cd_left - p.dash_cd_left).abs() > 0.05
        || (c.melee_cd_left - p.melee_cd_left).abs() > 0.05
        || (c.stun_cd_left - p.stun_cd_left).abs() > 0.05
        || (c.attack_lock_left - p.attack_lock_left).abs() > 0.05
        || (c.stun_left - p.stun_left).abs() > 0.05
        || (c.dash_left - p.dash_left).abs() > 0.05
        || (c.block_charge - p.block_charge).abs() > 0.05
}

/// Плагин протокола: регистрирует реплицируемые компоненты, нативный инпут и
/// сообщения. Добавляется в app ПОСЛЕ Client/ServerPlugins (на обеих сторонах
/// протокол обязан совпадать).
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // --- Канал служебных сообщений (надёжный, упорядоченный) ---
        app.add_channel::<crate::auth::AuthChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..Default::default()
        })
        .add_direction(NetworkDirection::Bidirectional);

        // --- Канал FX (ненадёжный, неупорядоченный): разовые эффекты/звуки ---
        app.add_channel::<crate::fx::FxChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,
            ..Default::default()
        })
        .add_direction(NetworkDirection::ServerToClient);

        // --- Реплицируемые компоненты ---
        // ВАЖНО (модель синхронизации Lightyear): на ПРЕДСКАЗАННУЮ сущность
        // (локальный игрок) попадают все реплицируемые компоненты, а на
        // ИНТЕРПОЛИРУЕМУЮ (чужие игроки, неписи, гранаты) — ТОЛЬКО те, у которых
        // зарегистрирована интерполяция. Поэтому всё геймплейное состояние, которое
        // клиентский «мост» читает с интерполируемых сущностей (тип неписи, HP,
        // способности, блок, маркер игрока/гранаты), регистрируем с интерполяцией.
        // Для дискретного состояния (не презентация) используем `snap` — без
        // сглаживания, просто доставка значения на интерполируемую копию.
        app.register_component::<Player>()
            .add_interpolation_with(snap::<Player>);
        app.register_component::<PlayerName>().add_interpolation_with(snap::<PlayerName>);
        // Презентация: предсказываем у локального игрока, интерполируем у прочих.
        app.register_component::<Position>()
            .add_prediction()
            .add_should_rollback(pos_should_rollback)
            .add_linear_interpolation()
            .add_linear_correction_fn();
        // Rotation БЕЗ correction-фн: коррекция визуально «долизывала» поворот
        // ~200 мс после каждого отката — прицел ощущался ватным. Реслимуляция
        // отката и так даёт почти точный угол; редкий снап на ≥3° незаметен.
        app.register_component::<Rotation>()
            .add_prediction()
            .add_should_rollback(rot_should_rollback)
            .add_linear_interpolation();
        // Геймплейное состояние: предсказываем у локального игрока (rollback) и
        // снап-«интерполируем» у удалённых, чтобы HP/способности/блок были доступны
        // на интерполируемых сущностях (полоски HP, анимации удара/рывка, атрибуция
        // урона).
        app.register_component::<Health>()
            .add_prediction()
            .add_interpolation_with(snap::<Health>);
        app.register_component::<AbilityState>()
            .add_prediction()
            .add_should_rollback(abil_should_rollback)
            .add_interpolation_with(snap::<AbilityState>);
        app.register_component::<Blocking>()
            .add_prediction()
            .add_interpolation_with(snap::<Blocking>);
        // Непись — авторитет ИИ на сервере; клиенту нужны тип/runtime/HP на
        // интерполируемой копии (отрисовка спрайта, полоска HP, агро/атака).
        app.register_component::<Npc>()
            .add_interpolation_with(snap::<Npc>);
        app.register_component::<NpcRuntime>()
            .add_interpolation_with(snap::<NpcRuntime>);
        // Граната — серверная сущность; клиенту нужны позиция (интерполяция) и
        // маркер `Grenade` на интерполируемой копии (для навешивания визуала).
        app.register_component::<Grenade>()
            .add_interpolation_with(snap::<Grenade>);

        // --- Пользовательский ввод (tick-synced) ---
        app.add_plugins(NativeInputPlugin::<NetInput>::default());

        // --- Сообщения авторизации / таблицы очков ---
        app.register_message::<Hello>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<AuthOk>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<AuthDenied>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<Scoreboard>()
            .add_direction(NetworkDirection::ServerToClient);

        // --- FX: one-shot события через event-репликацию Lightyear (не message) ---
        app.register_event::<crate::messages::Fx>()
            .add_direction(NetworkDirection::ServerToClient);
    }
}
