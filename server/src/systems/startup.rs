use std::net::{IpAddr, Ipv4Addr};
use bevy::prelude::*;
use bevy_quinnet::server::{
    EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
    ServerEndpointConfigurationDefaultables,
};
use bevy_quinnet::server::certificate::CertificateRetrievalMode;
use crate::net::channels_config;

pub fn start_server(mut server: ResMut<QuinnetServer>) {
    let endpoint_cfg = ServerEndpointConfiguration {
        addr_config: EndpointAddrConfiguration::from_ip(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            6000,
        ),
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
    println!("✅ Server started on 127.0.0.1:6000");
}
