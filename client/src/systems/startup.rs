use crate::resources::UiFont;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;

pub fn setup(mut commands: Commands, mut client: ResMut<QuinnetClient>) {
    commands.spawn(Camera2d::default());
}

pub fn load_ui_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("fonts/FiraSans-Bold.ttf");
    commands.insert_resource(UiFont(handle));
}
