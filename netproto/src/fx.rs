//! Разовые FX/звуковые события сервера → клиентам через Lightyear event-репликацию.
//!
//! Системы боя/гранат/ИИ кладут FX в очередь `FxOut` в течение тика; `flush_fx`
//! сливает её в `EventSender<Fx>` на каждом подключённом клиенте (канал
//! `FxChannel`, unreliable). Клиент получает `RemoteEvent<Fx>` и проигрывает
//! звук/визуал.

use bevy::prelude::*;
use lightyear::prelude::*;

use crate::messages::Fx;

/// Канал для FX: ненадёжный неупорядоченный (потеря отдельного эффекта не
/// критична, зато нет ретрансмиссии и блокировок).
pub struct FxChannel;

/// Очередь FX, накопленных за текущий тик; очищается после рассылки.
#[derive(Resource, Default)]
pub struct FxOut(pub Vec<Fx>);

impl FxOut {
    #[inline]
    pub fn push(&mut self, fx: Fx) {
        self.0.push(fx);
    }
}

/// Сливает накопленные FX в `EventSender` всех подключённых клиентов.
pub fn flush_fx(
    mut out: ResMut<FxOut>,
    mut senders: Query<&mut EventSender<Fx>, With<Connected>>,
) {
    if out.0.is_empty() {
        return;
    }
    for fx in out.0.drain(..) {
        for mut s in &mut senders {
            s.trigger::<FxChannel>(fx);
        }
    }
}
