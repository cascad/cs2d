use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{constants::CH_S2C, messages::S2C};

use crate::resources::{PlayerStates, RespawnDelay, RespawnQueue, RespawnTask};

/// Событие урона: любой источник пишет сюда
#[derive(Event)]
pub struct DamageEvent {
    pub target: u64,
    pub amount: i32,
    pub source: Option<u64>,
}

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
        println!("[DEBUG] damage event {:?}- {:?}", ev.target, ev.amount);
        if let Some(st) = states.0.get_mut(&ev.target) {
            println!("[DEBUG] states {:?}", st.hp);
            st.hp -= ev.amount;
            
            info!(
                "🩸 Player {} took {} dmg (hp={})",
                ev.target, ev.amount, st.hp
            );
            println!(
                "🩸 Player {} took {} dmg (hp={})",
                ev.target, ev.amount, st.hp
            );

            if st.hp <= 0 {
                // 1) сразу рассылаем PlayerDied
                server
                    .endpoint_mut()
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
