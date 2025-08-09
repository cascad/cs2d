use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::CH_S2C;
use protocol::messages::S2C;

use crate::app_state::AppState;
use crate::menu::ConnectTimeout;
use crate::resources::CurrentConnId;

pub fn connecting_pump(
    mut client: ResMut<QuinnetClient>,
    conn_id: Option<Res<CurrentConnId>>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let Some(id) = conn_id.and_then(|c| c.0) else {
        return;
    };
    let Some(mut conn) = client.get_connection_mut() else {
        return;
    };

    // Читаем только чтобы поймать первый Snapshot. Остальные сообщения можно игнорить.
    while let Some((chan, msg)) = conn.try_receive_message::<S2C>() {
        if chan != CH_S2C {
            continue;
        }
        match msg {
            S2C::Snapshot(_snap) => {
                // Сигнал «сервер жив» — снимаем таймер и заходим в игру.
                commands.remove_resource::<ConnectTimeout>();
                info!("✅ Got first Snapshot, entering InGame");
                next.set(AppState::InGame);
                // Первый снап мы не используем здесь — норм, в InGame его схватит обычная сеть.
                break;
            }
            _ => { /* игнор */ }
        }
    }
}
