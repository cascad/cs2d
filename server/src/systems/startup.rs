use bevy::prelude::*;
use bevy_quinnet::server::{
    EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
    ServerEndpointConfigurationDefaultables,
};
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use crate::config::ServerConfig;
use crate::net::{channels_config, start_meta_endpoint};
use crate::resources::ConnectedClients;
use std::sync::atomic::Ordering;

pub fn start_server(mut commands: Commands, mut server: ResMut<QuinnetServer>, cfg: Res<ServerConfig>) {
    // Адрес/порт берём из конфига рядом с бинарём (по умолчанию 0.0.0.0:6000).
    // 0.0.0.0 — все интерфейсы (LAN/интернет); 127.0.0.1 — только локально.
    let endpoint_cfg = ServerEndpointConfiguration {
        addr_config: EndpointAddrConfiguration::from_ip(cfg.ip_addr(), cfg.port),
        cert_mode: CertificateRetrievalMode::GenerateSelfSigned {
            server_hostname: "localhost".into(),
        },
        defaultables: ServerEndpointConfigurationDefaultables {
            send_channels_cfg: channels_config(),
            ..Default::default()
        },
    };
    if let Err(e) = server.start_endpoint(endpoint_cfg) {
        error!("failed to start server endpoint: {e:?}");
        return;
    }
    println!("✅ Server started on {}:{}", cfg.ip, cfg.port);

    // Фоновый TCP-листенер меты для мини-лобби клиента (тот же номер порта).
    let meta = start_meta_endpoint(cfg.ip_addr(), cfg.port, cfg.name.clone(), cfg.max_players);
    commands.insert_resource(meta);
}

/// Обновляет разделяемый счётчик игроков, который читает мета-листенер при
/// ответе на query-запросы из лобби.
pub fn update_meta_player_count(
    clients: Res<ConnectedClients>,
    meta: Option<Res<crate::net::MetaPlayerCount>>,
) {
    if let Some(meta) = meta {
        meta.0.store(clients.0.len() as u32, Ordering::Relaxed);
    }
}
