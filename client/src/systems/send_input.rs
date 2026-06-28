use crate::components::{ActorAnim, AnimState, Facing, LocalPlayer};
use crate::render::WorldPos;
use crate::resources::{
    AimAngle, CurrentStance, LocalAbilities, PendingInputsClient, PredictedPos, SendTimer,
    SeqCounter, WallGridRes,
};
use crate::systems::melee::spawn_player_melee_decal;
use crate::systems::utils::time_in_seconds;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::abilities::{tick_abilities, AbilityConfig, AbilityInput};
use protocol::constants::{CH_C2S, PLAYER_SIZE, TICK_DT};
use protocol::messages::{C2S, InputState};

/// Ресурсы для спавна декали собственного взмаха. Упакованы в один `SystemParam`,
/// чтобы `send_input_and_predict` не превысил лимит Bevy на 16 аргументов системы.
#[derive(SystemParam)]
pub struct MeleeFxCtx<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<ColorMaterial>>,
}

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
    // визуал собственного взмаха (декаль на полу) предсказываем локально;
    // commands+meshes+materials упакованы в один SystemParam, чтобы не упереться
    // в лимит Bevy на 16 аргументов системы
    mut fx: MeleeFxCtx,
    // накапливаем нажатие рывка между тиками, чтобы не потерять его,
    // если Space нажали не в кадр отправки
    mut dash_latch: Local<bool>,
    // то же для удара: ЛКМ могли кликнуть не в кадр отправки
    mut attack_latch: Local<bool>,
) {
    let dash_now = keys.just_pressed(KeyCode::Space);
    *dash_latch |= dash_now;
    *attack_latch |= mouse.just_pressed(MouseButton::Left);

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
    // удар — ЛКМ (удержание = серия ударов по готовности КД) или одиночный клик,
    // пойманный латчем между тиками
    let want_attack = *attack_latch || mouse.pressed(MouseButton::Left);

    let cfg = AbilityConfig::default();
    let out = tick_abilities(
        &mut abilities.0,
        &AbilityInput {
            move_dir: dir,
            aim: aim_angle,
            want_dash: *dash_latch,
            want_block,
            want_attack,
        },
        TICK_DT,
        &cfg,
    );

    // фактический (плавно довёрнутый) угол модели — его и рисуем/шлём
    let facing = out.facing;
    player_facing.0 = facing;

    anim.blocking = out.blocking;

    // Перекат предсказываем ЛОКАЛЬНО из того же детерминированного тика, что и его
    // ДВИЖЕНИЕ: анимация и рывок рождаются из одного `did_dash`, поэтому не могут
    // рассинхрониться («быстро проехал без анимации» был именно таким разъездом —
    // движение предсказывалось, а анимацию ждали с сервера). КД/стамину проверяет
    // `tick_abilities`, так что на КД рывок вообще не стартует. Серверный DashFx с
    // нашим id клиент игнорирует (см. network.rs), чтобы не дублировать визуал.
    if out.did_dash {
        let ang = if out.dash_dir.length_squared() > 1e-6 {
            out.dash_dir.y.atan2(out.dash_dir.x)
        } else {
            facing
        };
        anim.start_action_facing(AnimState::Dash, ang);
    }

    // Собственный взмах предсказываем локально (отзывчивость): анимация + декаль
    // зоны удара. Удар — часть детерминированного тика, поэтому `did_attack`
    // совпадает с серверным; урон считает сервер, а его MeleeFx с нашим id клиент
    // игнорирует, чтобы не дублировать визуал.
    if out.did_attack {
        anim.start_action(AnimState::Attack);
        spawn_player_melee_decal(
            &mut fx.commands,
            &mut fx.meshes,
            &mut fx.materials,
            predicted.pos,
            out.attack_dir,
        );
    }

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
        attack: out.did_attack,
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
    *attack_latch = false;
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
