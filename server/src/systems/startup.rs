use std::net::{IpAddr, Ipv4Addr};
use bevy::prelude::*;
use bevy_quinnet::server::{QuinnetServer, ServerEndpointConfiguration};
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use crate::net::channels_config;

pub fn start_server(mut server: ResMut<QuinnetServer>) {
    let endpoint_cfg = ServerEndpointConfiguration::from_ip(
        IpAddr::V4(Ipv4Addr::new(127,0,0,1)),
        6000,
        // idle_timeout_ms: Some(3000), // 3 секунды до отключения неактивного соединения
        // keep_alive_interval_ms: Some(1000), // Отправлять keep-alive каждую секунду
        // max_idle_timeout_ms: Some(5000), // Максимальный таймаут 5 секунд
    );
    let cert_mode = CertificateRetrievalMode::GenerateSelfSigned { server_hostname: "localhost".into() };
    let channels = channels_config();
    server.start_endpoint(endpoint_cfg, cert_mode, channels).unwrap();
    println!("✅ Server started on 127.0.0.1:6000");
}
