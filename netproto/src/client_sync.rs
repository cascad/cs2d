//! «Разморозка» задержки ввода — обход храповика lightyear 0.26 (клиентская
//! сторона; общая для нативного и wasm клиента, покрыта интеграционным тестом
//! `netproto/tests/unstable_ping.rs`).
//!
//! Lightyear пересчитывает input_delay ТОЛЬКО при жёстком ресинке
//! (`recompute_input_delay_on_sync` по `SyncEvent`), а завышенная задержка сама
//! гасит сигнал ресинка: `sync_objective` вычитает задержку и клампится на
//! remote+1 (lightyear_sync-0.26.4/src/timeline/input.rs:289-291), после чего
//! ошибка таймлайна всегда ~0 и ресинк не наступает никогда. Итог: замер RTT,
//! отравленный джанком страницы в окне рукопожатия (один pong, отвисевший 2.5 с
//! в замороженной вкладке, даёт задержку ~300 тиков ≈ 5 с) или временной
//! просадкой сети, ЗАМЕРЗАЕТ до конца сессии — хотя сама EWMA-оценка RTT
//! выздоравливает за секунды. Симптом: «предикта нет», любой ввод отрабатывает
//! через секунды при идеальном текущем rtt.
//!
//! Обход userland (без вендоринга lightyear_sync): повторная вставка
//! `InputTimelineConfig` штатно триггерит у lightyear наблюдатель
//! `recompute_input_delay_on_config_update` (input.rs:74-90), который
//! пересчитывает задержку из ТЕКУЩИХ link.stats. Система [`refresh_input_delay`]
//! раз в секунду сравнивает применяемую задержку с расчётом по живой оценке и
//! при завышении переустанавливает конфиг. Рост задержки (реальное ухудшение
//! сети) оставляем родному механизму ресинка — вверх не дёргаем.

use core::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::client::{InputDelayConfig, InputTimeline, InputTimelineConfig};
use lightyear::prelude::{Client, Connected, IsSynced, Link, SyncConfig};

/// Активная политика задержки ввода/синка клиента. Вставляется клиентом при
/// старте ТЕМИ ЖЕ значениями, что уходят в `InputTimelineConfig` при коннекте —
/// [`refresh_input_delay`] пересоздаёт конфиг из неё.
#[derive(Resource, Clone)]
pub struct InputDelayPolicy {
    pub delay: InputDelayConfig,
    pub sync: SyncConfig,
}

impl InputDelayPolicy {
    pub fn new(delay: InputDelayConfig, sync: SyncConfig) -> Self {
        Self { delay, sync }
    }
}

/// Зеркало приватной формулы lightyear `InputDelayConfig::input_delay_ticks`
/// (lightyear_sync-0.26.4/src/timeline/input.rs:209-246, lockstep у нас нет):
/// сколько тиков задержки ввода полагается при данных RTT/джиттере.
pub fn expected_delay_ticks(
    delay: &InputDelayConfig,
    sync: &SyncConfig,
    rtt: Duration,
    jitter: Duration,
) -> u16 {
    let eff = rtt + jitter * sync.jitter_multiple as u32 + sync.jitter_margin;
    let rtt_ticks = (eff.as_secs_f32() / protocol::constants::TICK_DT).ceil() as u16;
    if rtt_ticks <= delay.minimum_input_delay_ticks {
        delay.minimum_input_delay_ticks
    } else if rtt_ticks <= delay.maximum_input_delay_before_prediction {
        rtt_ticks
    } else if rtt_ticks
        <= delay.maximum_predicted_ticks + delay.maximum_input_delay_before_prediction
    {
        delay.maximum_input_delay_before_prediction
    } else {
        rtt_ticks - delay.maximum_predicted_ticks
    }
}

/// Раз в секунду: если применяемая задержка ввода завышена относительно живой
/// оценки RTT/джиттера — форсируем пересчёт повторной вставкой конфига.
/// Без [`InputDelayPolicy`] в мире система молчит (сервер/тесты без фикса).
pub fn refresh_input_delay(
    time: Res<Time>,
    policy: Option<Res<InputDelayPolicy>>,
    mut last: Local<f64>,
    clients: Query<
        (Entity, &InputTimeline, &Link),
        (With<Client>, With<Connected>, With<IsSynced<InputTimeline>>),
    >,
    mut commands: Commands,
) {
    let Some(policy) = policy else { return };
    let now = time.elapsed_secs_f64();
    if now - *last < 1.0 {
        return;
    }
    *last = now;
    let Ok((entity, timeline, link)) = clients.single() else {
        return;
    };
    let want = expected_delay_ticks(&policy.delay, &policy.sync, link.stats.rtt, link.stats.jitter);
    let cur = timeline.input_delay();
    // Гистерезис 2 тика: дыхание джиттера не должно дёргать буфер ввода.
    if cur > want.saturating_add(2) {
        warn!(
            "[ly] задержка ввода залипла: {cur} тиков при ожидаемых {want} \
             (rtt={:.0}ms jit={:.0}ms) — пересчитываем",
            link.stats.rtt.as_secs_f64() * 1000.0,
            link.stats.jitter.as_secs_f64() * 1000.0,
        );
        commands.entity(entity).insert(InputTimelineConfig::new(
            policy.sync.clone(),
            policy.delay.clone(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(min: u16, before_pred: u16, max_pred: u16) -> InputDelayConfig {
        InputDelayConfig {
            minimum_input_delay_ticks: min,
            maximum_input_delay_before_prediction: before_pred,
            maximum_predicted_ticks: max_pred,
        }
    }

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn sync() -> SyncConfig {
        SyncConfig::default() // jitter_multiple 4, jitter_margin 5мс
    }

    /// Сверка зеркала с формулой lightyear (input.rs:209-246) на конфиге {2,3,7}
    /// (прежний wasm): значения из аналитического разбора lightyear_sync-0.26.4.
    #[test]
    fn mirrors_lightyear_formula_237() {
        let c = cfg(2, 3, 7);
        // eff = rtt + 4*jit + 5мс; тик 15 мс.
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(10), MS(0)), 2); // 15мс → 1 тик → min
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(50), MS(0)), 3); // 55мс → 4 тика → cap 3
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(300), MS(0)), 14); // 305мс → 21 → 21-7
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(1000), MS(0)), 60); // 1005мс → 67 → 67-7
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(3000), MS(0)), 194); // ≈2.9с задержки
        // Джиттер первого сэмпла = RTT/4 (посев оценщика): eff ≈ 2·RTT + 5мс.
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(3000), MS(750)), 394); // ≈5.9с
    }

    /// Текущий wasm-конфиг {2,3,20}: плохой Wi-Fi (RTT до ~340мс) больше не
    /// наращивает задержку — излишек уходит в предсказание.
    #[test]
    fn wasm_config_absorbs_bad_wifi() {
        let c = cfg(2, 3, 20);
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(50), MS(2)), 3);
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(300), MS(5)), 3); // 325мс → 22 ≤ 23
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(340), MS(0)), 3); // 345мс → 23 ≤ 23
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(400), MS(0)), 7); // 405мс → 27 → 27-20
    }

    /// Нативный balanced {0,3,7}: на localhost задержка ~1-3 тика.
    #[test]
    fn native_balanced_low_rtt() {
        let c = InputDelayConfig::balanced();
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(1), MS(0)), 1); // 6мс → 1 тик
        assert_eq!(expected_delay_ticks(&c, &sync(), MS(25), MS(2)), 3); // 38мс → 3
    }
}
