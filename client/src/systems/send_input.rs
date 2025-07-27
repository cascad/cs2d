use crate::components::LocalPlayer;
use crate::resources::{
    CurrentStance, PendingInputsClient, SendTimer, SeqCounter,
};
use crate::systems::utils::simulate_input;
use crate::systems::utils::time_in_seconds;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::constants::CH_C2S;
use protocol::messages::{C2S, InputState};

pub fn send_input_and_predict(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<SendTimer>,
    mut client: ResMut<QuinnetClient>,
    stance: Res<CurrentStance>,
    mut seq: ResMut<SeqCounter>,
    mut pending: ResMut<PendingInputsClient>,
    mut q: Query<&mut Transform, With<LocalPlayer>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    if let Ok(mut t) = q.single_mut() {
        seq.0 = seq.0.wrapping_add(1);
        let inp = InputState {
            seq: seq.0,
            up: keys.pressed(KeyCode::KeyW),
            down: keys.pressed(KeyCode::KeyS),
            left: keys.pressed(KeyCode::KeyA),
            right: keys.pressed(KeyCode::KeyD),
            rotation: t.rotation.to_euler(EulerRot::XYZ).2,
            stance: stance.0.clone(),
            timestamp: time_in_seconds(),
        };
        client
            .connection_mut()
            .send_message_on(CH_C2S, C2S::Input(inp.clone()))
            .ok();
        pending.0.push_back(inp.clone());
        if pending.0.len() > 256 {
            pending.0.pop_front();
        }
        simulate_input(&mut *t, &inp);
    }
}
