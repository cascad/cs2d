use crate::resources::CurrentConnId;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use bevy_quinnet::client::certificate::CertificateVerificationMode;
use bevy_quinnet::client::connection::ClientEndpointConfiguration;
use protocol::quinnet_adapter::build_channels_config;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub fn setup(mut commands: Commands, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d::default());

    let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();
    let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    let cert_mode = CertificateVerificationMode::SkipVerification;

    let channels_config = build_channels_config();
    let conn_id = client
        .open_connection(endpoint_config, cert_mode, channels_config)
        .expect("Couldn't open connection");

    // сохраняем ID в ресурсе
    commands.insert_resource(CurrentConnId(Some(conn_id)));
}
