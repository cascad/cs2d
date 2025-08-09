use crate::events::DamageEvent;
use crate::resources::{GrenadeSyncTimer, Grenades, PlayerStates};
use crate::systems::wall::Wall;
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::constants::{
    CH_S2C, GRENADE_AIR_DRAG_PER_SEC, GRENADE_BLAST_RADIUS, GRENADE_BOUNCE_DAMPING,
    GRENADE_DAMAGE_COEFF, GRENADE_RADIUS, GRENADE_RESTITUTION, GRENADE_STOP_SPEED, MAX_STEP,
    SEPARATION_EPS,
};
use protocol::messages::S2C;

// ---- утилиты коллизии -------------------------------------------------------

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
    wt: &Transform,
    sprite: &Sprite,
) -> Option<(Vec2, Vec2)> {
    let size = sprite.custom_size?;
    let half_w = size / 2.0;
    let rect_center = wt.translation.truncate();
    let min_b = rect_center - half_w;
    let max_b = rect_center + half_w;

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
    wall_q: &Query<(&Transform, &Sprite), With<Wall>>,
) -> Option<(Vec2, Vec2)> {
    for (wt, sprite) in wall_q.iter() {
        if let Some(hit) = collide_circle_with_wall_precise(center, r, wt, sprite) {
            return Some(hit);
        }
    }
    None
}

// ---- основная система -------------------------------------------------------

/// Обновляем гранаты: полёт с отскоком + взрыв по таймеру (без Rapier)
pub fn update_grenades(
    mut grenades: ResMut<Grenades>,
    states: Res<PlayerStates>,
    mut damage_events: EventWriter<DamageEvent>,
    time: Res<Time>,
    wall_q: Query<(&Transform, &Sprite), With<Wall>>,
    mut server: ResMut<QuinnetServer>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();

    // --- Полёт + отскоки ---
    for (&_id, gs) in grenades.0.iter_mut() {
        // Текущее положение из вашей формулы-«якоря» (не меняем created)
        let t_since_created = (now - gs.created) as f32;
        let mut pos = gs.ev.from + gs.ev.dir * gs.ev.speed * t_since_created;

        // Если в стартовой позиции уже внутри стенки (игрок вплотную к стене) — мягко вытолкнем
        if let Some((_n0, corr0)) = collide_circle_with_walls(pos, GRENADE_RADIUS, &wall_q) {
            pos += corr0;
        }

        // Воздушное затухание
        gs.ev.speed *= (1.0 - GRENADE_AIR_DRAG_PER_SEC).powf(dt);
        if gs.ev.speed < GRENADE_STOP_SPEED {
            gs.ev.speed = 0.0;
        }

        // Перемещение за этот кадр
        let mut remaining = gs.ev.dir * gs.ev.speed * dt;

        // Локальные скорость/направление (чтобы сохранить семантику твоих полей)
        let mut dir = gs.ev.dir;
        let mut speed = gs.ev.speed;

        let mut bounced = false;

        // Пошаговое движение
        while remaining.length_squared() > 0.0 {
            let step_len = remaining.length().min(MAX_STEP);
            let step_dir = remaining.normalize_or_zero();
            let step = step_dir * step_len;

            let proposed = pos + step;

            if let Some((normal, corr)) =
                collide_circle_with_walls(proposed, GRENADE_RADIUS, &wall_q)
            {
                // выталкиваем до касания (без «зазора»)
                let contact_pos = proposed + corr;
                pos = contact_pos;

                // отражаем скорость относительно нормали + затухание
                let v = dir * speed;

                let reflected = v - 2.0 * v.dot(normal) * normal;
                let new_speed = reflected.length() * GRENADE_RESTITUTION * GRENADE_BOUNCE_DAMPING;

                let new_dir = if new_speed > 0.0 {
                    reflected / new_speed
                } else {
                    dir
                };

                dir = new_dir;
                speed = if new_speed < GRENADE_STOP_SPEED {
                    0.0
                } else {
                    new_speed
                };

                bounced = true;
                // после удара останавливаем перенос в этом кадре — стабильнее и без «заеданий»
                break;
            } else {
                pos = proposed;
                remaining -= step;
            }
        }

        // Обновляем параметры «якорной» формулы, НЕ трогая created (таймер взрыва прежний).
        gs.ev.dir = dir;
        gs.ev.speed = speed;
        gs.ev.from = pos - dir * speed * t_since_created;

        // Мгновенная синхра при рикошете (чтобы клиент сразу «схлопнулся» на новую траекторию)
        if bounced {
            let ep = server.endpoint_mut();
            let vel = dir * speed;
            let _ = ep.broadcast_message_on(
                CH_S2C,
                &S2C::GrenadeSync {
                    id: gs.ev.id,
                    pos,
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
