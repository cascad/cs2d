//! TLS-сертификат для WebTransport (self-signed в dev, PEM-файлы в prod).

use bevy::prelude::*;
use lightyear::prelude::Identity;

use crate::config::ServerConfig;

/// Готовый TLS identity + hex-digest первого сертификата (для wasm-клиента).
pub struct WtIdentity {
    pub identity: Identity,
    pub digest: String,
}

impl WtIdentity {
    /// Строит identity из конфига: auto self-signed SAN (dev) или PEM (prod).
    pub fn from_config(cfg: &ServerConfig) -> Self {
        let identity = if let (Some(cert), Some(key)) = (&cfg.tls_cert_pem, &cfg.tls_key_pem) {
            info!("🔐 WebTransport: загрузка PEM {cert} + {key}");
            // sync-обёртка: сервер headless, IoTaskPool Bevy тут не поднимаем.
            load_pem_blocking(cert, key)
        } else {
            let sans = cfg.webtransport_sans.clone();
            info!("🔐 WebTransport: self-signed SANs {:?}", sans);
            Identity::self_signed(sans).expect("self-signed identity")
        };
        let digest = identity.certificate_chain().as_slice()[0].hash().to_string();
        info!("🔐 WebTransport certificate digest (wasm client / URL #hash): {digest}");
        Self { identity, digest }
    }
}

fn load_pem_blocking(cert: &str, key: &str) -> Identity {
    std::thread::scope(|s| {
        s.spawn(|| {
            let rt = tokio::runtime::Runtime::new().expect("tokio rt");
            rt.block_on(Identity::load_pemfiles(cert, key))
                .expect("load PEM identity")
        })
        .join()
        .expect("pem loader thread")
    })
}

/// Пишет digest в `certificates/digest.txt` (удобно перед `trunk build`).
pub fn write_digest_file(digest: &str) {
    let path = std::path::Path::new("certificates/digest.txt");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, digest) {
        warn!("не удалось записать {}: {e}", path.display());
    } else {
        info!("🔐 digest записан в {}", path.display());
    }
}
