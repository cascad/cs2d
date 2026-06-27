pub mod abilities;
pub mod combat;
pub mod constants;
pub mod channels;
pub mod geom;
pub mod level;
pub mod maps;
pub mod messages;
pub mod server_meta;

// Адаптер для Quinnet (включать с фичей "quinnet")
#[cfg(feature = "quinnet")]
pub mod quinnet_adapter;