use crate::components::LocalPlayer;
use crate::render::WorldPos;
use crate::resources::{
    CurrentStance, LocalAbilities, PendingInputsClient, PredictedPos, SendTimer, SeqCounter,
    WallGridRes,
};
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::abilities::{tick_abilities, AbilityConfig, AbilityInput};
use protocol::constants::{CH_C2S, PLAYER_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

/// Предсказание движения локального игрока в ЛОКСТЕПЕ с отправкой ввода:
/// симуляция шагает ровно раз в тик (как сервер), а не каждый кадр. Это убирает
/// рассинхрон каденса «кадры≠тики» (из-за которого позиция дёргалась при
/// реконсиляции) и делает скорость независимой от FPS. Transform здесь НЕ
/// двигаем — отрисовку плавно подтягивает `smooth_local_player`.
pub fn send_input_and_predict(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
    stance: Res<CurrentStance>,
    mut seq: ResMut<SeqCounter>,
    mut pending: ResMut<PendingInputsClient>,
    walls: Res<WallGridRes>,
    mut abilities: ResMut<LocalAbilities>,
    mut predicted: ResMut<PredictedPos>,
    player_q: Query<&Transform, With<LocalPlayer>>,
    // накапливаем нажатие рывка между тиками, чтобы не потерять его,
    // если Space нажали не в кадр отправки
    mut dash_latch: Local<bool>,
) {
    let dash_now = keys.just_pressed(KeyCode::Space);
    *dash_latch |= dash_now;

    // шагаем симуляцию строго раз в тик
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    if !predicted.valid {
        return; // ждём первую авторитетную позицию из снапшота
    }

    let Ok(tf) = player_q.single() else { return };
    let facing = tf.rotation.to_euler(EulerRot::XYZ).2;

    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { dir.y += 1.; }
    if keys.pressed(KeyCode::KeyS) { dir.y -= 1.; }
    if keys.pressed(KeyCode::KeyA) { dir.x -= 1.; }
    if keys.pressed(KeyCode::KeyD) { dir.x += 1.; }
    dir = dir.normalize_or_zero();

    // блок — удержание mouse2
    let want_block = mouse.pressed(MouseButton::Right);

    let cfg = AbilityConfig::default();
    let out = tick_abilities(
        &mut abilities.0,
        &AbilityInput {
            move_dir: dir,
            facing,
            want_dash: *dash_latch,
            want_block,
        },
        TICK_DT,
        &cfg,
    );

    let delta = out.move_dir * out.speed * TICK_DT;
    predicted.pos = walls.0.slide_circle(predicted.pos, delta, PLAYER_SIZE * 0.5);

    seq.0 = seq.0.wrapping_add(1);
    let inp = InputState {
        seq: seq.0,
        up: keys.pressed(KeyCode::KeyW),
        down: keys.pressed(KeyCode::KeyS),
        left: keys.pressed(KeyCode::KeyA),
        right: keys.pressed(KeyCode::KeyD),
        rotation: facing,
        stance: stance.0.clone(),
        timestamp: time_in_seconds(),
        block: want_block,
        dash: *dash_latch,
    };
    client
        .connection_mut()
        .send_message_on(CH_C2S, C2S::Input(inp.clone()))
        .ok();
    pending.0.push_back(inp);
    if pending.0.len() > 256 {
        pending.0.pop_front();
    }
    *dash_latch = false;
}

/// Плавно подтягивает ОТРИСОВКУ локального игрока к точной предсказанной позиции.
/// Симуляция шагает раз в тик (64 Гц), а кадров обычно больше — без сглаживания
/// это выглядело бы ступенчато. Чисто визуально: на симуляцию/сеть/других
/// клиентов не влияет. Большой скачок (телепорт/респавн) применяем мгновенно.
pub fn smooth_local_player(
    time: Res<Time>,
    predicted: Res<PredictedPos>,
    mut q: Query<&mut WorldPos, With<LocalPlayer>>,
) {
    if !predicted.valid {
        return;
    }
    let Ok(mut wp) = q.single_mut() else { return };
    let cur = wp.0;
    let target = predicted.pos;

    let new = if (target - cur).length() > 256.0 {
        target // далеко (респавн/коррекция) — без скольжения
    } else {
        // экспоненциальное приближение, не зависящее от FPS
        let k = 30.0;
        let a = 1.0 - (-k * time.delta_secs()).exp();
        cur.lerp(target, a)
    };
    wp.0 = new;
}
