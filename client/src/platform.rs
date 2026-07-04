//! Кроссплатформенные хелперы: время и случайность без `std::time`/`process::id`
//! (на wasm32 они недоступны или ведут себя иначе).

/// Монотонные наносекунды с эпохи (для сидов/идентификаторов).
#[inline]
pub fn now_nanos() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

/// Псевдослучайное u64 (не криптостойкое — достаточно для client_id/аудио).
#[inline]
pub fn random_u64() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        let hi = (js_sys::Math::random() * u32::MAX as f64) as u32;
        let lo = (js_sys::Math::random() * u32::MAX as f64) as u32;
        ((hi as u64) << 32) | lo as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        nanos ^ (std::process::id() as u64).rotate_left(32)
    }
}

/// Секунды с эпохи (для таймеров/утилит).
#[inline]
pub fn now_secs_f64() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
}
