use crate::resources::PlayerState;
use crate::resources::PlayerStates;
use crate::resources::RespawnQueue;
use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use bevy_quinnet::server::{ConnectionEvent, ConnectionLostEvent};
use protocol::constants::CH_S2C;
use protocol::messages::PlayerSnapshot;
use protocol::messages::Stance;
use protocol::messages::WorldSnapshot;
use protocol::messages::S2C;

/// Ловим ивенты подключения/отключения клиента
// pub fn handle_connection_events(
//     mut connect_ev: EventReader<ConnectionEvent>,
//     mut lost_ev: EventReader<ConnectionLostEvent>,
//     mut states: ResMut<PlayerStates>,
//     mut respawns: ResMut<RespawnQueue>,
// ) {
//     // 1) новые подключения
//     for ev in connect_ev.read() {
//         let id = ev.id;
//         info!("[Server] client {} connected", id);
//         // убираем старые задачи респавна
//         respawns.0.retain(|t| t.pid != id);
//         // вставляем дефолтное состояние, чтобы он появился в первом снапшоте
//         states.0.insert(
//             id,
//             PlayerState {
//                 pos: Vec2::ZERO, // или сразу pick_spawn_point()
//                 rot: 0.0,
//                 stance: Stance::Standing,
//                 hp: 100,
//             },
//         );
//     }

//     // 2) отключения
//     for ev in lost_ev.read() {
//         let id = ev.id;
//         info!("[Server] client {} disconnected", id);
//         // удаляем из активных состояний
//         states.0.remove(&id);
//         // и из очереди респавнов
//         respawns.0.retain(|t| t.pid != id);
//     }
// }

pub fn handle_connection_events(
    mut connect_ev: EventReader<ConnectionEvent>,
    mut lost_ev:    EventReader<ConnectionLostEvent>,
    mut states:     ResMut<PlayerStates>,
    mut respawns:   ResMut<RespawnQueue>,
    mut server:     ResMut<QuinnetServer>,
    time:           Res<Time>,
) {
    // 1) Новые подключения
    for ConnectionEvent { id } in connect_ev.read() {
        info!("[Server] client {} connected", id);

        // чистим старые задачи respawn
        respawns.0.retain(|t| t.pid != *id);

        // вставляем дефолтное состояние, чтобы он появился в snapshot
        states.0.insert(
            *id,
            PlayerState {
                pos:    Vec2::ZERO, // или pick_spawn_point(id)
                rot:    0.0,
                stance: Stance::Standing,
                hp:     100,
            },
        );

        // и сразу же отсылаем этому клиенту полный current snapshot:
        let snapshot = {
            let players = states
                .0
                .iter()
                .map(|(&pid, st)| PlayerSnapshot {
                    id:       pid,
                    x:        st.pos.x,
                    y:        st.pos.y,
                    rotation: st.rot,
                    stance:   st.stance.clone(),
                    hp:       st.hp,
                })
                .collect();
            WorldSnapshot {
                players,
                server_time: time.elapsed_secs_f64(),
                last_input_seq: std::collections::HashMap::new(), // или текущее applied.seq
            }
        };
        server
            .endpoint_mut()
            .send_message_on(*id, CH_S2C, S2C::Snapshot(snapshot))
            .unwrap();
        info!("[Server] sent initial Snapshot to {}", id);
    }

    // 2) Отключения
    for ConnectionLostEvent { id } in lost_ev.read() {
        info!("[Server] client {} disconnected", id);
        states.0.remove(id);
        respawns.0.retain(|t| t.pid != *id);
    }
}