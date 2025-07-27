use crate::systems::disconnect::GoodbyeSent;
use bevy::prelude::*; // ресурс‑флаг из send_goodbye_and_close

/// На следующем кадре после Goodbye посылаем AppExit
pub fn exit_if_goodbye_done(flag: Res<GoodbyeSent>, mut exitw: EventWriter<AppExit>) {
    if flag.0 {
        exitw.write(AppExit::Success);
    }
}
