use crate::components::{ActorAnim, Facing, LocalPlayer};
use crate::render::WorldPos;
use crate::resources::{
    AimAngle, CurrentStance, LocalAbilities, PendingInputsClient, PredictedPos, SendTimer,
    SeqCounter, WallGridRes,
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
    aim: Res<AimAngle>,
    mut player_q: Query<(&mut Facing, &mut ActorAnim), With<LocalPlayer>>,
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

    let Ok((mut player_facing, mut anim)) = player_q.single_mut() else { return };
    // желаемый угол на курсор; фактический (довёрнутый) вернёт tick_abilities
    let aim_angle = aim.0;

    // Изо-ремап управления: WASD трактуем как ЭКРАННЫЕ направления и переводим в
    // мир. При изо 2:1 экран-вверх = world(1,1), вниз = (-1,-1), влево = (-1,1),
    // вправо = (1,-1). Затем сводим к знакам по осям мира — сервер из этих же
    // булевых посчитает ту же нормированную скорость (рассинхрона нет).
    let mut v = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { v += Vec2::new(1.0, 1.0); }
    if keys.pressed(KeyCode::KeyS) { v += Vec2::new(-1.0, -1.0); }
    if keys.pressed(KeyCode::KeyA) { v += Vec2::new(-1.0, 1.0); }
    if keys.pressed(KeyCode::KeyD) { v += Vec2::new(1.0, -1.0); }
    let in_up = v.y > 0.0;
    let in_down = v.y < 0.0;
    let in_left = v.x < 0.0;
    let in_right = v.x > 0.0;

    let mut dir = Vec2::ZERO;
    if in_up { dir.y += 1.; }
    if in_down { dir.y -= 1.; }
    if in_left { dir.x -= 1.; }
    if in_right { dir.x += 1.; }
    dir = dir.normalize_or_zero();

    // блок — удержание mouse2
    let want_block = mouse.pressed(MouseButton::Right);

    let cfg = AbilityConfig::default();
    let out = tick_abilities(
        &mut abilities.0,
        &AbilityInput {
            move_dir: dir,
            aim: aim_angle,
            want_dash: *dash_latch,
            want_block,
        },
        TICK_DT,
        &cfg,
    );

    // фактический (плавно довёрнутый) угол модели — его и рисуем/шлём
    let facing = out.facing;
    player_facing.0 = facing;

    // Анимацию переката НЕ заводим из предсказания — её надёжно включает событие
    // `S2C::DashFx` от сервера (иначе при расхождении стамины/кулдауна ролл
    // иногда не проигрывался). Блок — держится, пока установлен (mouse2).
    anim.blocking = out.blocking;

    // Мировая скорость этого тика — её отрисовка интегрирует КАЖДЫЙ КАДР
    // (экстраполяция), поэтому движение по экрану ровное и не зависит от каденса
    // «кадры vs тики». При коллизии slide_circle мог урезать шаг — берём ФАКТИЧЕСКИ
    // применённую скорость, чтобы у стены отрисовка не «пёрла» сквозь неё.
    let delta = out.move_dir * out.speed * TICK_DT;
    let new_pos = walls.0.slide_circle(predicted.pos, delta, PLAYER_SIZE * 0.5);
    predicted.vel = (new_pos - predicted.pos) / TICK_DT;
    predicted.pos = new_pos;

    seq.0 = seq.0.wrapping_add(1);
    let inp = InputState {
        seq: seq.0,
        up: in_up,
        down: in_down,
        left: in_left,
        right: in_right,
        // шлём ЖЕЛАЕМЫЙ угол (курсор) — сервер довернёт модель тем же кодом
        rotation: aim_angle,
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

/// Плавно ведёт ОТРИСОВКУ локального игрока методом dead-reckoning: каждый кадр
/// продвигаем позицию по последней известной СКОРОСТИ (экстраполяция) и мягко
/// стягиваем к авторитетной `predicted.pos`. Используем ТОЛЬКО часы движка
/// (`time.delta`), без рассинхрона «настенные часы vs тик» — поэтому на прямой
/// скорость постоянная и рывки незаметны глазу даже при будущем пинге, а
/// коррекция реконсиляции «размазывается» и не дёргает. Большой скачок
/// (телепорт/респавн) применяем мгновенно.
pub fn smooth_local_player(
    time: Res<Time>,
    predicted: Res<PredictedPos>,
    mut q: Query<&mut WorldPos, With<LocalPlayer>>,
) {
    if !predicted.valid {
        return;
    }
    let Ok(mut wp) = q.single_mut() else { return };
    let dt = time.delta_secs();
    let cur = wp.0;

    // далёкий скачок — это телепорт/респавн: ставим мгновенно
    if (predicted.pos - cur).length() > 256.0 {
        wp.0 = predicted.pos;
        return;
    }

    // 1) экстраполяция по скорости (ровно, каждый кадр)
    let extrap = cur + predicted.vel * dt;
    // 2) мягкая коррекция к авторитетной позиции (гасит дрейф/реконсиляцию).
    //    k небольшой → коррекция плавная и незаметная; скорость остаётся ровной.
    let k = 12.0;
    let a = 1.0 - (-k * dt).exp();
    wp.0 = extrap.lerp(predicted.pos, a);
}
