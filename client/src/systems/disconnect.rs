// systems/disconnect.rs
use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use bevy_quinnet::client::QuinnetClient;
use protocol::messages::C2S;
use protocol::constants::CH_C2S;
use crate::resources::CurrentConnId;

/// Флаг, чтобы не дублировать
#[derive(Resource, Default)]
pub struct GoodbyeSent(pub bool);

pub fn send_goodbye_and_close(
    mut ev_win: EventReader<WindowCloseRequested>,
    keys: Res<ButtonInput<KeyCode>>,
    mut client: ResMut<QuinnetClient>,
    conn_id: Res<CurrentConnId>,
    mut flag: ResMut<GoodbyeSent>,
) {
    if flag.0 {
        return;
    }

    // крестик или Esc
    if ev_win.read().next().is_some() || keys.just_pressed(KeyCode::Escape) {
        if let Some(id) = conn_id.0 {
            // посылаем Goodbye
            let _ = client
                .connection_mut()
                .send_message_on(CH_C2S, C2S::Goodbye);
            // закрываем UDP‑сокет: это _сразу_ шлёт FIN/RESET по DTLS
            let _ = client.close_connection(id);
            info!("👋 Goodbye sent, socket closed");
            flag.0 = true;           // помечаем, что всё сделали
        }
    }
}
