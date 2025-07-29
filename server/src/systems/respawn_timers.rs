use bevy::prelude::*;
use crate::{
    events::PlayerRespawn,
    resources::RespawnQueue,
};

/// Проверяем все задачи респавна и по истечении таймера
/// шлём внутриресурсное событие PlayerRespawn
pub fn process_respawn_timers(
    mut respawn_q:  ResMut<RespawnQueue>,
    mut respawn_ev: EventWriter<PlayerRespawn>,
    time:            Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    // Собираем все готовые
    let mut ready = Vec::new();
    respawn_q.0.retain(|task| {
        if now >= task.due {
            ready.push((task.pid, task.pos));
            false
        } else {
            true
        }
    });
    // Выстреливаем событие для каждого
    for (pid, pos) in ready {
        respawn_ev.write(PlayerRespawn { id: pid, x: pos.x, y: pos.y });
    }
}
