use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{BLOCK_DAMAGE_MULT, CH_S2C},
    messages::S2C,
};

use crate::{
    events::DamageEvent,
    resources::{PlayerStates, RespawnDelay, RespawnQueue, RespawnTask},
};

pub fn apply_damage(
    mut ev_damage: EventReader<DamageEvent>,
    mut states: ResMut<PlayerStates>,
    mut respawn_q: ResMut<RespawnQueue>,
    delay: Res<RespawnDelay>,
    time: Res<Time>,
    mut server: ResMut<QuinnetServer>,
) {
    let now = time.elapsed_secs_f64();
    for ev in ev_damage.read() {
        // println!("[DEBUG] damage event player:{:?} {:?}", ev.target, ev.amount);
        if let Some(st) = states.0.get_mut(&ev.target) {
            // активный блок снижает входящий урон
            let amount = if st.blocking {
                ((ev.amount as f32) * BLOCK_DAMAGE_MULT).round() as i32
            } else {
                ev.amount
            };
            st.hp -= amount;

            // todo not work info! here
            println!(
                "🩸 Player {} took {} dmg (hp={}, blocking={})",
                ev.target, amount, st.hp, st.blocking
            );

            let endpoint = server.endpoint_mut();

            // send damage event
            endpoint
                .broadcast_message_on(
                    CH_S2C,
                    S2C::PlayerDamaged {
                        id: ev.target,
                        new_hp: st.hp,
                        damage: amount,
                    },
                )
                .ok();

            if st.hp <= 0 {
                // 1) сразу рассылаем PlayerDied
                endpoint
                    .broadcast_message_on(
                        CH_S2C,
                        S2C::PlayerDied {
                            victim: ev.target,
                            killer: ev.source,
                        },
                    )
                    .unwrap();
                info!("💀 [Server] Player {} died", ev.target);

                // 2) удаляем состояние и планируем респавн
                states.0.remove(&ev.target);

                // ОЧИЩАЕМ предыдущие задачи для этого игрока
                respawn_q.0.retain(|task| task.pid != ev.target);

                // ставим задачу на время now + delay
                let spawn_pos = pick_spawn_point(ev.target);
                respawn_q.0.push(RespawnTask {
                    pid: ev.target,
                    due: now + delay.0,
                    pos: spawn_pos,
                });

                info!(
                    "⏳ [Server] Scheduled respawn of {} at t={}",
                    ev.target,
                    now + delay.0
                );
            }
        }
    }
}

// todo загружать логику спавнов отдельно
/// Простая функция, возвращающая точку спавна по ID.
/// Замените логику на свою: рандом, круг, свободные точки и т.д.
fn pick_spawn_point(pid: u64) -> Vec2 {
    const POINTS: [Vec2; 4] = [
        Vec2::new(-300.0, -200.0),
        Vec2::new(300.0, -200.0),
        Vec2::new(-300.0, 200.0),
        Vec2::new(300.0, 200.0),
    ];
    let idx = (pid as usize) % POINTS.len();
    POINTS[idx]
}
