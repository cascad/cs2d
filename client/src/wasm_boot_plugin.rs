//! Пустой плагин-заглушка на native; на wasm делегирует в `wasm_boot`.

use bevy::prelude::*;

pub struct WasmBootPlugin;

#[cfg(target_arch = "wasm32")]
impl Plugin for WasmBootPlugin {
    fn build(&self, app: &mut App) {
        crate::wasm_boot::WasmBootPlugin.build(app);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Plugin for WasmBootPlugin {
    fn build(&self, _app: &mut App) {}
}
