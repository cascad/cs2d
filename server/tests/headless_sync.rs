//! Headless-интеграционный тест связки «предсказание клиента ↔ авторитет сервера».
//!
//! Поднимаем ДВА bevy-`App` на `MinimalPlugins` (серверный и клиентский), гоняем
//! N тиков, перекидывая сообщения через in-memory транспорт с искусственной
//! задержкой (вместо quinnet — он и так протестирован библиотекой), и проверяем,
//! что после прекращения ввода предсказанная позиция клиента СХОДИТСЯ к
//! авторитетной позиции сервера. Движение/коллизия/реконсиляция используют ту же
//! общую функцию `protocol::level::slide_move`, что и боевой код.

use std::collections::{HashSet, VecDeque};

use bevy::prelude::*;
use protocol::constants::{MOVE_SPEED, PLAYER_SIZE, TICK_DT, TILE_SIZE};
use protocol::level::{rasterize_walls, slide_move};

const HALF: f32 = PLAYER_SIZE * 0.5;

#[derive(Clone, Copy)]
struct Input {
    seq: u32,
    dir: Vec2,
}

#[derive(Clone, Copy)]
struct Snapshot {
    pos: Vec2,
    last_seq: u32,
}

// ----------------------------- серверный App --------------------------------

#[derive(Resource)]
struct SvWorld {
    pos: Vec2,
    last_seq: u32,
    solids: HashSet<IVec2>,
}

#[derive(Resource, Default)]
struct SvInbox(Vec<Input>);

#[derive(Resource, Default)]
struct SvOutbox(Option<Snapshot>);

fn server_step(mut w: ResMut<SvWorld>, mut inbox: ResMut<SvInbox>, mut out: ResMut<SvOutbox>) {
    let mut inputs = std::mem::take(&mut inbox.0);
    inputs.sort_by_key(|i| i.seq);
    for inp in inputs {
        let delta = inp.dir * MOVE_SPEED * TICK_DT;
        let next = slide_move(w.pos, delta, &w.solids, HALF, TILE_SIZE);
        w.pos = next;
        w.last_seq = inp.seq;
    }
    out.0 = Some(Snapshot {
        pos: w.pos,
        last_seq: w.last_seq,
    });
}

// ----------------------------- клиентский App -------------------------------

#[derive(Resource)]
struct ClWorld {
    predicted: Vec2,
    solids: HashSet<IVec2>,
    pending: VecDeque<Input>,
    seq: u32,
}

#[derive(Resource, Default)]
struct ClHeld(Vec2);

#[derive(Resource, Default)]
struct ClOutbox(Vec<Input>);

#[derive(Resource, Default)]
struct ClSnapshot(Option<Snapshot>);

/// Реконсиляция: при получении снапшота откатываемся к авторитетной позиции и
/// переигрываем ещё не подтверждённые вводы.
fn client_reconcile(mut w: ResMut<ClWorld>, mut snap: ResMut<ClSnapshot>) {
    let Some(s) = snap.0.take() else { return };
    while let Some(front) = w.pending.front() {
        if front.seq <= s.last_seq {
            w.pending.pop_front();
        } else {
            break;
        }
    }
    let mut pos = s.pos;
    let pend: Vec<Input> = w.pending.iter().copied().collect();
    for inp in pend {
        let delta = inp.dir * MOVE_SPEED * TICK_DT;
        pos = slide_move(pos, delta, &w.solids, HALF, TILE_SIZE);
    }
    w.predicted = pos;
}

/// Локальное предсказание: применяем текущий ввод и кладём его в pending + отправку.
fn client_predict(mut w: ResMut<ClWorld>, held: Res<ClHeld>, mut out: ResMut<ClOutbox>) {
    w.seq += 1;
    let inp = Input {
        seq: w.seq,
        dir: held.0,
    };
    let delta = inp.dir * MOVE_SPEED * TICK_DT;
    let next = slide_move(w.predicted, delta, &w.solids, HALF, TILE_SIZE);
    w.predicted = next;
    w.pending.push_back(inp);
    out.0.push(inp);
}

// ------------------------------ сам тест ------------------------------------

fn make_solids() -> HashSet<IVec2> {
    // вертикальная стена-полоса справа: мир [64,96] x [-64,64]
    let mut set = HashSet::new();
    rasterize_walls(
        &mut set,
        &[(Vec2::new(64.0, -64.0), Vec2::new(96.0, 64.0))],
        TILE_SIZE,
    );
    set
}

fn build_server() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(SvWorld {
        pos: Vec2::ZERO,
        last_seq: 0,
        solids: make_solids(),
    });
    app.init_resource::<SvInbox>();
    app.init_resource::<SvOutbox>();
    app.add_systems(Update, server_step);
    app
}

fn build_client() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(ClWorld {
        predicted: Vec2::ZERO,
        solids: make_solids(),
        pending: VecDeque::new(),
        seq: 0,
    });
    app.init_resource::<ClHeld>();
    app.init_resource::<ClOutbox>();
    app.init_resource::<ClSnapshot>();
    // сначала реконсиляция (если пришёл снапшот), затем предсказание нового ввода
    app.add_systems(Update, (client_reconcile, client_predict).chain());
    app
}

#[test]
fn client_prediction_converges_to_server_authority() {
    let mut server = build_server();
    let mut client = build_client();

    const LAG: usize = 2; // задержка доставки в тиках (в каждую сторону)
    const MOVE_TICKS: usize = 40;
    const TOTAL: usize = MOVE_TICKS + 4 * LAG + 10;

    // буферы задержки: (тик_доставки, payload)
    let mut to_server: VecDeque<(usize, Vec<Input>)> = VecDeque::new();
    let mut to_client: VecDeque<(usize, Snapshot)> = VecDeque::new();

    // движение по диагонали вправо-вверх — упрётся в стену по X, проскользнёт по Y
    let move_dir = Vec2::new(1.0, 0.4).normalize();

    for tick in 0..TOTAL {
        // 1. задаём ввод (двигаемся первые MOVE_TICKS, потом стоп)
        client.world_mut().resource_mut::<ClHeld>().0 =
            if tick < MOVE_TICKS { move_dir } else { Vec2::ZERO };

        // 2. доставляем клиенту снапшот, чей срок настал
        if let Some(&(due, snap)) = to_client.front() {
            if due <= tick {
                client.world_mut().resource_mut::<ClSnapshot>().0 = Some(snap);
                to_client.pop_front();
            }
        }

        // 3. тик клиента (реконсиляция + предсказание)
        client.update();

        // 4. собираем исходящие вводы клиента → в буфер к серверу
        let inputs = std::mem::take(&mut client.world_mut().resource_mut::<ClOutbox>().0);
        if !inputs.is_empty() {
            to_server.push_back((tick + LAG, inputs));
        }

        // 5. доставляем серверу вводы, чей срок настал
        if let Some(&(due, _)) = to_server.front() {
            if due <= tick {
                let (_, inputs) = to_server.pop_front().unwrap();
                server.world_mut().resource_mut::<SvInbox>().0.extend(inputs);
            }
        }

        // 6. тик сервера
        server.update();

        // 7. снапшот сервера → в буфер к клиенту
        if let Some(snap) = server.world_mut().resource_mut::<SvOutbox>().0.take() {
            to_client.push_back((tick + LAG, snap));
        }
    }

    // дренируем оставшиеся доставки, чтобы клиент догнал авторитет
    while let Some((_, snap)) = to_client.pop_front() {
        client.world_mut().resource_mut::<ClSnapshot>().0 = Some(snap);
        client.update();
    }

    let predicted = client.world().resource::<ClWorld>().predicted;
    let authority = server.world().resource::<SvWorld>().pos;

    // 1) сходимость: предсказание совпало с авторитетом
    assert!(
        (predicted - authority).length() < 1e-3,
        "клиент не сошёлся с сервером: predicted={predicted:?}, authority={authority:?}"
    );

    // 2) движение реально произошло
    assert!(authority.length() > 1.0, "игрок не сдвинулся: {authority:?}");

    // 3) коллизия сработала одинаково: по X упёрлись в стену (≈ 64 - HALF), не прошли насквозь
    assert!(
        authority.x < 64.0 - HALF + 0.5,
        "игрок не должен был пройти сквозь стену по X: {authority:?}"
    );
    assert!(authority.y > 1.0, "по Y игрок должен был проскользнуть: {authority:?}");
}
