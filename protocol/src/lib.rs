pub mod constants;
pub mod channels;
pub mod messages;

// Адаптер для Quinnet (включать с фичей "quinnet")
#[cfg(feature = "quinnet")]
pub mod quinnet_adapter;