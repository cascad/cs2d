//! TLS-сертификат для WebTransport (self-signed в dev/на игровом порту, PEM в prod).
//!
//! Self-signed сертификат ПЕРСИСТЕНТНЫЙ: генерится не на каждый старт, а раз в
//! ~`CERT_ROTATE_SECS`, ключ+серт хранятся в `certificates/` (в docker — том
//! `./data/certificates`). Рестарт/деплой/крэш сервера НЕ меняет сертификат —
//! уже загруженные страницы игроков продолжают подключаться без F5. Chrome
//! принимает hash-валидацию только для сертификатов со сроком ≤ 14 дней,
//! поэтому ротация раз в ~6 дней (еженедельный авторестарт контейнера
//! гарантирует, что ротация случится с запасом до истечения).

use std::path::Path;

use bevy::prelude::*;
use lightyear::prelude::Identity;

use crate::config::ServerConfig;

/// Возраст, после которого self-signed сертификат пересоздаётся (сек). Должен
/// быть заметно меньше 14 дней (срок жизни серта и лимит Chrome) минус период
/// авторестарта сервера (7 дней): 6д + 7д = 13д < 14д.
const CERT_ROTATE_SECS: u64 = 6 * 24 * 3600;

/// Готовый TLS identity + hex-digest первого сертификата (для wasm-клиента).
pub struct WtIdentity {
    pub identity: Identity,
    pub digest: String,
}

impl WtIdentity {
    /// Строит identity из конфига: PEM (prod) или персистентный self-signed.
    pub fn from_config(cfg: &ServerConfig) -> Self {
        let identity = if let (Some(cert), Some(key)) = (&cfg.tls_cert_pem, &cfg.tls_key_pem) {
            info!("🔐 WebTransport: загрузка PEM {cert} + {key}");
            load_pem_blocking(cert, key).expect("load PEM identity")
        } else {
            persistent_self_signed(&cfg.webtransport_sans)
        };
        let digest = identity.certificate_chain().as_slice()[0].hash().to_string();
        info!("🔐 WebTransport certificate digest (wasm client / URL #hash): {digest}");
        Self { identity, digest }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Загружает сохранённый self-signed сертификат, если он моложе
/// [`CERT_ROTATE_SECS`], иначе генерит новый и сохраняет (ключ+серт+штамп).
fn persistent_self_signed(sans: &[String]) -> Identity {
    let dir = Path::new("certificates");
    let cert_p = dir.join("wt_cert.pem");
    let key_p = dir.join("wt_key.pem");
    let stamp_p = dir.join("wt_created");

    let age = std::fs::read_to_string(&stamp_p)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ts| now_unix().saturating_sub(ts));
    if let Some(age) = age.filter(|a| *a < CERT_ROTATE_SECS) {
        if cert_p.exists() && key_p.exists() {
            match load_pem_blocking(&cert_p.to_string_lossy(), &key_p.to_string_lossy()) {
                Ok(identity) => {
                    info!(
                        "🔐 WebTransport: переиспользуем сохранённый сертификат (возраст {:.1} дн., ротация после {:.0} дн.)",
                        age as f64 / 86_400.0,
                        CERT_ROTATE_SECS as f64 / 86_400.0
                    );
                    return identity;
                }
                Err(e) => warn!("🔐 сохранённый сертификат не загрузился ({e}) — генерируем новый"),
            }
        }
    }

    info!("🔐 WebTransport: новый self-signed, SANs {sans:?} (сохраняем для будущих рестартов)");
    let identity = Identity::self_signed(sans).expect("self-signed identity");
    let _ = std::fs::create_dir_all(dir);
    let chain_pem: String = identity
        .certificate_chain()
        .as_slice()
        .iter()
        .map(|c| c.to_pem())
        .collect();
    let key_pem = identity.private_key().to_secret_pem();
    // Порядок важен: штамп пишем ПОСЛЕДНИМ — если запись оборвалась, следующий
    // старт увидит старый/отсутствующий штамп и просто перегенерит.
    if let Err(e) = std::fs::write(&cert_p, chain_pem)
        .and_then(|_| std::fs::write(&key_p, key_pem))
        .and_then(|_| std::fs::write(&stamp_p, now_unix().to_string()))
    {
        warn!("🔐 не удалось сохранить сертификат ({e}) — рестарт сгенерит новый (игрокам потребуется F5)");
    }
    identity
}

fn load_pem_blocking(cert: &str, key: &str) -> Result<Identity, String> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            rt.block_on(Identity::load_pemfiles(cert, key))
                .map_err(|e| e.to_string())
        })
        .join()
        .map_err(|_| "pem loader thread panicked".to_string())?
    })
}

/// Пишет digest в `certificates/digest.txt` и, если рядом есть собранный
/// `dist/` (локальная разработка с `trunk serve`), — сразу и в
/// `dist/certificates/digest.txt`: trunk отдаёт файлы копией на момент сборки,
/// и без живой перезаписи браузер после смены сертификата фетчил протухший
/// digest до полной (минуты в release) пересборки. В docker `dist/` в контейнере
/// game отсутствует — запись просто пропускается (там живой файл отдаёт Caddy
/// из общего тома).
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
    let dist = std::path::Path::new("dist/certificates");
    if dist.parent().is_some_and(|d| d.exists()) {
        let _ = std::fs::create_dir_all(dist);
        if std::fs::write(dist.join("digest.txt"), digest).is_ok() {
            info!("🔐 digest обновлён и в dist/ (для trunk serve)");
        }
    }
}
