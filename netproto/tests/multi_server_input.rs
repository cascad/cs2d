//! Применяются ли клиентские инпуты на сервере, когда в мире ДВЕ Started-сущности
//! сервера (UDP + WebTransport), как в server/src/main.rs.
//!
//! Штатная `lightyear_inputs::server::update_action_state` берёт сервер через
//! `Single<(Entity, Has<HostServer>), With<Started>>`: при двух Started система
//! молча пропускается — ActionState никогда не обновляется из InputBuffer, сервер
//! симулирует игроков с пустым вводом (рубербендинг у всех клиентов). Обход —
//! `netproto::apply_player_inputs`, регистрируется сервером в FixedPreUpdate
//! (см. server/src/main.rs); здесь он проверяется в той же конфигурации.

use core::time::Duration;

use bevy::prelude::*;
use lightyear::input::input_buffer::InputBuffer;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use netproto::{NetInput, ProtocolPlugin, PRIVATE_KEY, PROTOCOL_ID};

fn run_case(num_servers: usize, with_workaround: bool) -> f32 {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(ServerPlugins {
        tick_duration: netproto::tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);
    if with_workaround {
        // как в server/src/main.rs
        app.add_systems(FixedPreUpdate, netproto::apply_player_inputs);
    }

    // Слушатели, как в server/src/main.rs (без IO: Started ставится по Start).
    let netcode_cfg = NetcodeConfig {
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        ..default()
    };
    for i in 0..num_servers {
        let e = app
            .world_mut()
            .spawn((
                Name::from(format!("Server{i}")),
                NetcodeServer::new(netcode_cfg.clone()),
            ))
            .id();
        app.world_mut().trigger(Start { entity: e });
    }

    // «Игрок» с заполненным InputBuffer (как будто инпуты уже пришли от клиента).
    let mut buffer = InputBuffer::<ActionState<NetInput>, NetInput>::default();
    let want = NetInput {
        up: true,
        aim: 5.0,
        ..Default::default()
    };
    for t in 0..2000u16 {
        buffer.set(lightyear::prelude::Tick(t), ActionState(want));
    }
    let player = app
        .world_mut()
        .spawn((ActionState::<NetInput>::default(), buffer))
        .id();

    // Прогоняем ~50 тиков реального времени (tick 15 мс).
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(16));
        app.update();
    }

    let st = app
        .world()
        .get::<ActionState<NetInput>>(player)
        .expect("ActionState remains");
    st.0.aim
}

#[test]
fn one_started_server_applies_inputs() {
    let aim = run_case(1, false);
    assert_eq!(aim, 5.0, "с одним Started-сервером штатный путь lightyear работает");
}

#[test]
fn two_started_servers_apply_inputs_with_workaround() {
    let aim = run_case(2, true);
    assert_eq!(
        aim, 5.0,
        "с двумя Started-серверами (UDP+WebTransport) ввод обязан применяться \
         через netproto::apply_player_inputs — как в server/src/main.rs"
    );
}

/// КАНАРЕЙКА апстримного бага: штатная update_action_state при двух Started
/// молча пропускается (Single не резолвится). Когда этот тест УПАДЁТ — значит,
/// новая версия lightyear починила Single, и `netproto::apply_player_inputs`
/// можно удалять (вместе с этим тестом).
#[test]
fn upstream_single_still_broken_with_two_servers() {
    let aim = run_case(2, false);
    assert_eq!(
        aim, 0.0,
        "апстрим починил Single<..., With<Started>>: workaround apply_player_inputs больше не нужен"
    );
}
