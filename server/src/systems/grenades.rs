use crate::events::DamageEvent;
use crate::resources::Grenades;
use crate::resources::PlayerStates;
use bevy::prelude::*;
use protocol::constants::GRENADE_BLAST_RADIUS;

/// Обновляем таймеры гранат и наносим урон при взрыве
pub fn update_grenades(
    mut grenades: ResMut<Grenades>,
    states: Res<PlayerStates>, // Позиции игроков
    mut damage_events: EventWriter<DamageEvent>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let mut to_explode = Vec::new();

    // Собираем гранаты, у которых истёк таймер
    for (&id, state) in grenades.0.iter() {
        if now - state.created >= state.ev.timer as f64 {
            to_explode.push(id);
        }
    }

    // Обрабатываем взрывы
    for id in to_explode {
        if let Some(gs) = grenades.0.remove(&id) {
            let ev = gs.ev;

            // 🔥 Расчёт текущей позиции гранаты
            let lifetime = (now - gs.created) as f32;
            let pos = ev.from + ev.dir * ev.speed * lifetime;

            info!("💥 Grenade {} exploded at {:?}", ev.id, pos);

            // Проверка расстояния до всех игроков
            for (&pid, pst) in states.0.iter() {
                let dist = (pst.pos - pos).length();
                let radius = GRENADE_BLAST_RADIUS;

                if dist <= radius {
                    let damage = ((radius - dist) / radius * 50.0) as i32;

                    info!(
                        "💥 → Player {} is within radius ({:.1}). Damage = {}",
                        pid, dist, damage
                    );

                    damage_events.write(DamageEvent {
                        target: pid,
                        amount: damage,
                        source: Some(ev.id),
                    });
                }
            }
        }
    }
}
