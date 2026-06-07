use crate::events::DamageEvent;
use crate::resources::{GrenadeSyncTimer, Grenades, PlayerStates, WallAabbs, WallGridRes};
use bevy::prelude::*;
use protocol::geom::WallGrid;
use bevy_quinnet::server::QuinnetServer;
use protocol::constants::{
    CH_S2C, GRENADE_AIR_DRAG_PER_SEC, GRENADE_BLAST_RADIUS, GRENADE_BOUNCE_DAMPING,
    GRENADE_DAMAGE_COEFF, GRENADE_RADIUS, GRENADE_RESTITUTION, GRENADE_STOP_SPEED, MAX_STEP,
    SEPARATION_EPS,
};
use protocol::messages::S2C;

// ---- основная система -------------------------------------------------------

/// Обновляем гранаты: полёт с отскоком + взрыв по таймеру (без Rapier)
pub fn update_grenades(
    mut grenades: ResMut<Grenades>,
    states: Res<PlayerStates>,
    mut damage_events: EventWriter<DamageEvent>,
    time: Res<Time>,
    walls: Res<WallAabbs>,
    wall_grid: Res<WallGridRes>,
    mut server: ResMut<QuinnetServer>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();

    // --- Полёт + отскоки ---
    for (&_id, gs) in grenades.0.iter_mut() {
        let phys = GrenadePhys {
            from: gs.ev.from,
            dir: gs.ev.dir,
            speed: gs.ev.speed,
            // время с момента создания на начало тика (не трогаем created)
            elapsed: (now - gs.created) as f32,
        };

        let res = step_grenade(&phys, dt, &walls.0);

        // Обновляем параметры «якорной» формулы, НЕ трогая created (таймер взрыва прежний).
        gs.ev.dir = res.state.dir;
        gs.ev.speed = res.state.speed;
        gs.ev.from = res.state.from;

        // Мгновенная синхра при рикошете (чтобы клиент сразу «схлопнулся» на новую траекторию)
        if res.bounced {
            let ep = server.endpoint_mut();
            let vel = res.state.dir * res.state.speed;
            let _ = ep.broadcast_message_on(
                CH_S2C,
                &S2C::GrenadeSync {
                    id: gs.ev.id,
                    pos: res.pos,
                    vel,
                    ts: now,
                },
            );
        }
    }

    // --- Взрывы по таймеру ---
    let mut to_explode = Vec::new();
    for (&id, state) in grenades.0.iter() {
        if now - state.created >= state.ev.timer as f64 {
            to_explode.push(id);
        }
    }

    // --- Нанесение урона и удаление ---
    for &id in &to_explode {
        if let Some(gs) = grenades.0.remove(&id) {
            let lifetime = (now - gs.created) as f32;
            let pos = gs.ev.from + gs.ev.dir * gs.ev.speed * lifetime;

            // Сообщаем всем клиентам точку детонации
            let ep = server.endpoint_mut();

            let _ = ep.broadcast_message_on(CH_S2C, &S2C::GrenadeDetonated { id: gs.ev.id, pos });

            info!("💥 Grenade {} exploded at {:?}", gs.ev.id, pos);

            for (&pid, pst) in states.0.iter() {
                let dist = (pst.pos - pos).length();
                if dist <= GRENADE_BLAST_RADIUS {
                    // проверка, не перекрыта ли линия взрыв→игрок стеной
                    if los_blocked_by_walls(pos, pst.pos, &wall_grid.0) {
                        // За стенкой — урон не проходит
                        continue;
                    }

                    let base_damage = ((GRENADE_BLAST_RADIUS - dist) / GRENADE_BLAST_RADIUS * 50.0)
                        * GRENADE_DAMAGE_COEFF;
                    damage_events.write(DamageEvent {
                        target: pid,
                        amount: base_damage as i32,
                        source: Some(gs.ev.id),
                    });
                }
            }
        }
    }
}

// ---- периодическая рассылка снапшотов --------------------------------------

pub fn broadcast_grenade_syncs(
    time: Res<Time>,
    mut sync_t: ResMut<GrenadeSyncTimer>,
    grenades: Res<Grenades>,
    mut server: ResMut<QuinnetServer>,
) {
    if !sync_t.0.tick(time.delta()).just_finished() {
        return;
    }
    let ep = server.endpoint_mut();

    let ts = time.elapsed_secs_f64();
    for (_id, gs) in grenades.0.iter() {
        // Текущая точка и скорость из вашей «якорной» формулы
        let t = (ts - gs.created) as f32;
        let pos = gs.ev.from + gs.ev.dir * gs.ev.speed * t;
        let vel = gs.ev.dir * gs.ev.speed; // уже с drag/демпфом, т.к. выше их обновляем

        let _ = ep.broadcast_message_on(
            CH_S2C,
            &S2C::GrenadeSync {
                id: gs.ev.id,
                pos,
                vel,
                ts,
            },
        );
    }
}

// ---- чистая физика одного тика гранаты (для golden-тестов) ------------------

/// Физическое состояние гранаты в «якорном» представлении.
/// `pos(t) = from + dir * speed * elapsed`, где `elapsed` — время с создания
/// на начало тика. Поля обновляются каждый тик, `created` (таймер взрыва) не трогаем.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrenadePhys {
    pub from: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    pub elapsed: f32,
}

/// Результат шага: новое состояние, текущая позиция и был ли отскок.
#[derive(Clone, Copy, Debug)]
pub struct GrenadeStepResult {
    pub state: GrenadePhys,
    pub pos: Vec2,
    pub bounced: bool,
}

/// Один тик физики гранаты: полёт с воздушным затуханием и отскоком от стен.
/// Чистая функция — без ECS/сети, чтобы её можно было прогнать в golden-тесте
/// и поймать любые изменения в физике отскоков.
pub fn step_grenade(s: &GrenadePhys, dt: f32, walls: &[(Vec2, Vec2)]) -> GrenadeStepResult {
    let t = s.elapsed;
    let mut pos = s.from + s.dir * s.speed * t;

    // Если стартовая позиция уже внутри стены — мягко выталкиваем.
    if let Some((_n0, corr0)) = collide_circle_with_walls(pos, GRENADE_RADIUS, walls) {
        pos += corr0;
    }

    // Воздушное затухание.
    let mut speed = s.speed * (1.0 - GRENADE_AIR_DRAG_PER_SEC).powf(dt);
    if speed < GRENADE_STOP_SPEED {
        speed = 0.0;
    }

    let mut dir = s.dir;
    let mut remaining = dir * speed * dt;
    let mut bounced = false;

    // Пошаговое движение с ограничением шага MAX_STEP.
    while remaining.length_squared() > 0.0 {
        let step_len = remaining.length().min(MAX_STEP);
        let step_dir = remaining.normalize_or_zero();
        let step = step_dir * step_len;

        let proposed = pos + step;

        if let Some((normal, corr)) = collide_circle_with_walls(proposed, GRENADE_RADIUS, walls) {
            // выталкиваем до касания (без «зазора»)
            pos = proposed + corr;

            // отражаем скорость относительно нормали + затухание
            let v = dir * speed;
            let reflected = v - 2.0 * v.dot(normal) * normal;
            let new_speed = reflected.length() * GRENADE_RESTITUTION * GRENADE_BOUNCE_DAMPING;

            dir = if new_speed > 0.0 { reflected / new_speed } else { dir };
            speed = if new_speed < GRENADE_STOP_SPEED { 0.0 } else { new_speed };

            bounced = true;
            // после удара останавливаем перенос в этом кадре — стабильнее
            break;
        } else {
            pos = proposed;
            remaining -= step;
        }
    }

    GrenadeStepResult {
        state: GrenadePhys {
            from: pos - dir * speed * t,
            dir,
            speed,
            elapsed: t + dt,
        },
        pos,
        bounced,
    }
}

// ---- утилиты коллизии -------------------------------------------------------

fn los_blocked_by_walls(p0: Vec2, p1: Vec2, walls: &WallGrid) -> bool {
    // небольшой зазор, чтобы не ловить «сам себя», если точка детонации лежит прямо на стене
    walls.segment_blocked(p0, p1, 0.001)
}

#[inline]
fn clamp_vec2(p: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
}

/// Точная коллизия круг (центр `c`, радиус `r`) против прямоугольника стены.
/// Возвращает (normal, correction), где:
/// - normal — наружная нормаль поверхности контакта;
/// - correction — вектор минимальной коррекции центра круга, чтобы выйти из пересечения.
fn collide_circle_with_wall_precise(
    c: Vec2,
    r: f32,
    min_b: Vec2,
    max_b: Vec2,
) -> Option<(Vec2, Vec2)> {
    // Ближайшая точка прямоугольника к центру круга
    let closest = clamp_vec2(c, min_b, max_b);
    let delta = c - closest;
    let d2 = delta.length_squared();

    if d2 > r * r {
        return None; // нет пересечения
    }

    // Есть пересечение.
    if d2 > 0.0 {
        // Обычный случай: ближайшая точка на стороне/углу → нормаль = нормализованный delta
        let dist = d2.sqrt();
        let n = delta / dist;
        // сколько нужно вытолкнуть из прямоугольника
        let push = (r - dist) + SEPARATION_EPS;
        return Some((n, n * push));
    }

    // Редкий случай: центр круга уже внутри прямоугольника (closest == c).
    // Выбираем ось с минимальным проникновением до стороны и толкаем по ней.
    let pen_left = (c.x - min_b.x).abs();
    let pen_right = (max_b.x - c.x).abs();
    let pen_bottom = (c.y - min_b.y).abs();
    let pen_top = (max_b.y - c.y).abs();

    let (n, push) = {
        let min_x = pen_left.min(pen_right);
        let min_y = pen_bottom.min(pen_top);
        if min_x < min_y {
            if pen_left < pen_right {
                (Vec2::NEG_X, (r + SEPARATION_EPS))
            } else {
                (Vec2::X, (r + SEPARATION_EPS))
            }
        } else {
            if pen_bottom < pen_top {
                (Vec2::NEG_Y, (r + SEPARATION_EPS))
            } else {
                (Vec2::Y, (r + SEPARATION_EPS))
            }
        }
    };
    Some((n, n * push))
}

/// Проверка по всем стенам: возвращает первую найденную нормаль и коррекцию
fn collide_circle_with_walls(
    center: Vec2,
    r: f32,
    walls: &[(Vec2, Vec2)],
) -> Option<(Vec2, Vec2)> {
    for &(min_b, max_b) in walls {
        if let Some(hit) = collide_circle_with_wall_precise(center, r, min_b, max_b) {
            return Some(hit);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALL_MIN: Vec2 = Vec2::new(0.0, 0.0);
    const WALL_MAX: Vec2 = Vec2::new(32.0, 32.0);

    #[test]
    fn circle_far_from_wall_no_collision() {
        assert!(
            collide_circle_with_wall_precise(Vec2::new(100.0, 100.0), 8.0, WALL_MIN, WALL_MAX)
                .is_none()
        );
    }

    #[test]
    fn circle_overlapping_side_pushes_outward() {
        // центр круга чуть правее правой грани стены
        let hit = collide_circle_with_wall_precise(Vec2::new(36.0, 16.0), 8.0, WALL_MIN, WALL_MAX);
        assert!(hit.is_some());
        let (normal, corr) = hit.unwrap();
        assert!(normal.x > 0.9, "нормаль должна смотреть вправо: {normal:?}");
        assert!(corr.x > 0.0, "коррекция должна выталкивать вправо: {corr:?}");
    }

    #[test]
    fn collide_scan_picks_overlapping_wall() {
        let walls = [
            (WALL_MIN, WALL_MAX),
            (Vec2::new(100.0, 100.0), Vec2::new(132.0, 132.0)),
        ];
        assert!(collide_circle_with_walls(Vec2::new(36.0, 16.0), 8.0, &walls).is_some());
        assert!(collide_circle_with_walls(Vec2::new(500.0, 500.0), 8.0, &walls).is_none());
    }

    // Сценарий golden-теста: граната летит вправо и отскакивает от стены.
    fn golden_walls() -> Vec<(Vec2, Vec2)> {
        vec![(Vec2::new(200.0, -64.0), Vec2::new(232.0, 64.0))]
    }

    fn golden_run(steps: usize) -> Vec<Vec2> {
        let dt = protocol::constants::TICK_DT;
        let walls = golden_walls();
        let mut s = GrenadePhys {
            from: Vec2::new(0.0, 0.0),
            dir: Vec2::new(1.0, 0.0),
            speed: 350.0,
            elapsed: 0.0,
        };
        let mut out = Vec::with_capacity(steps);
        for _ in 0..steps {
            let res = step_grenade(&s, dt, &walls);
            out.push(res.pos);
            s = res.state;
        }
        out
    }

    /// Записанный эталон траектории (40 шагов). Любое изменение физики
    /// отскоков/затухания/шага сдвинет эти числа и завалит тест.
    const GOLDEN: [(f32, f32); 40] = [
        (5.2451, 0.0),
        (15.7305, 0.0),
        (26.2062, 0.0),
        (36.6721, 0.0),
        (47.1284, 0.0),
        (57.5749, 0.0),
        (68.0118, 0.0),
        (78.4389, 0.0),
        (88.8564, 0.0),
        (99.2642, 0.0),
        (109.6624, 0.0),
        (120.0509, 0.0),
        (130.4298, 0.0),
        (140.7991, 0.0),
        (151.1587, 0.0),
        (161.5087, 0.0),
        (171.8492, 0.0),
        (182.1800, 0.0),
        (191.5000, 0.0),
        (181.1883, 0.0),
        (170.8862, 0.0),
        (160.5936, 0.0),
        (150.3107, 0.0),
        (140.0372, 0.0),
        (129.7733, 0.0),
        (119.5189, 0.0),
        (109.2740, 0.0),
        (99.0386, 0.0),
        (88.8127, 0.0),
        (78.5963, 0.0),
        (68.3893, 0.0),
        (58.1919, 0.0),
        (48.0038, 0.0),
        (37.8253, 0.0),
        (27.6562, 0.0),
        (17.4965, 0.0),
        (7.3463, 0.0),
        (-2.7946, 0.0),
        (-12.9260, 0.0),
        (-23.0480, 0.0),
    ];

    #[test]
    fn grenade_trajectory_matches_golden() {
        let traj = golden_run(GOLDEN.len());
        for (i, (p, &(gx, gy))) in traj.iter().zip(GOLDEN.iter()).enumerate() {
            assert!(
                (p.x - gx).abs() < 0.02 && (p.y - gy).abs() < 0.02,
                "шаг {i}: ожидалось ({gx:.4},{gy:.4}), получено ({:.4},{:.4}) — \
                 физика гранат изменилась",
                p.x,
                p.y
            );
        }
    }

    #[test]
    fn grenade_bounces_off_wall() {
        // качественная проверка: до стены летит вправо, после — влево
        let traj = golden_run(40);
        assert!(traj[10].x > traj[5].x, "до стены граната должна лететь вправо");
        assert!(traj[30].x < traj[20].x, "после стены — лететь обратно влево");
    }
}
