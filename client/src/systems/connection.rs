use crate::config::ClientConfig;
use crate::resources::{AuthState, MyPlayer};
use bevy::prelude::*;
use bevy_quinnet::client::connection::ConnectionEvent;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::CH_C2S;
use protocol::messages::C2S;

pub fn handle_connection_event(mut events: MessageReader<ConnectionEvent>, mut me: ResMut<MyPlayer>) {
    for ConnectionEvent { client_id, .. } in events.read() {
        if let Some(server_id) = client_id {
            if !me.got {
                me.id = *server_id;
                me.got = true;
                info!("[Client] got my id = {}", me.id);
            }
        }
    }
}

/// Сброс идентичности перед новым подключением: чтобы при реконнекте клиент
/// заново получил id и переотправил `Hello` (иначе остались бы старые значения).
pub fn reset_identity(mut me: ResMut<MyPlayer>, mut auth: ResMut<AuthState>) {
    me.id = 0;
    me.got = false;
    auth.sent_for = None;
}

/// Авторизация: как только узнали свой id, шлём `C2S::Hello` (имя+пароль из
/// конфига) ОДИН раз на соединение. Привязка к id обеспечивает переотправку при
/// реконнекте. Сервер по нему регистрирует/проверяет аккаунт и спавнит игрока.
pub fn send_hello(
    me: Res<MyPlayer>,
    cfg: Res<ClientConfig>,
    mut auth: ResMut<AuthState>,
    mut client: ResMut<QuinnetClient>,
) {
    if me.id == 0 || auth.sent_for == Some(me.id) {
        return;
    }
    let Some(conn) = client.get_connection_mut() else {
        return;
    };
    if conn
        .send_message_on(
            CH_C2S,
            C2S::Hello {
                name: cfg.name.clone(),
                password: cfg.password.clone(),
            },
        )
        .is_ok()
    {
        auth.sent_for = Some(me.id);
        info!("[Client] отправлен Hello как '{}'", cfg.name);
    }
}
