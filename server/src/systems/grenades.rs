use crate::resources::Grenades;
use crate::resources::PlayerStates;
use crate::systems::damage::DamageEvent;
use bevy::prelude::*;
use protocol::messages::GrenadeEvent;

/// Состояние брошенной гранаты
pub struct GrenadeState {
    pub ev: GrenadeEvent,
    pub created: f64,
}

/// Тикаем гранаты и отправляем события урона
pub fn update_grenades(
    mut grenades: ResMut<Grenades>,
    states: Res<PlayerStates>, // ← доступ к позициям игроков
    mut damage_events: EventWriter<DamageEvent>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let mut to_explode = Vec::new();

    // Собираем гранаты с истекшим таймером
    for (&id, st) in grenades.0.iter() {
        if now - st.created >= st.ev.timer as f64 {
            to_explode.push(id);
        }
    }

    // Для каждой взорвавшейся — считаем попадания и шлём DamageEvent
    for id in to_explode {
        if let Some(gs) = grenades.0.remove(&id) {
            let ev = gs.ev;
            // Пробегаем по всем живым игрокам
            for (&pid, pst) in states.0.iter() {
                let dist = (pst.pos - ev.from).length();
                if dist <= 100.0 {
                    let damage = ((100.0 - dist) / 100.0 * 50.0) as i32;
                    println!("  → Grenades AAAA {:?} - {:?}", pid, damage); 
                    damage_events.write(DamageEvent {
                        target: pid,
                        amount: damage,
                        source: Some(ev.id), // кто нанес
                    });
                }
            }
        }
    }
}
