use crate::resources::UiFont;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;

pub fn setup(mut commands: Commands, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d::default());

    // todo rm
    // let server_addr: SocketAddr = "127.0.0.1:6000".parse().unwrap();
    // let local_bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    // let endpoint_config = ClientEndpointConfiguration::from_addrs(server_addr, local_bind_addr);
    // let cert_mode = CertificateVerificationMode::SkipVerification;

    // let channels_config = build_channels_config();
    // let conn_id = client
    //     .open_connection(endpoint_config, cert_mode, channels_config)
    //     .expect("Couldn't open connection");

    // сохраняем ID в ресурсе
    // commands.insert_resource(CurrentConnId(Some(conn_id)));
}

pub fn load_ui_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.insert_resource(UiFont(handle));
}
