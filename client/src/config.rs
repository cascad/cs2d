//! Конфиг клиента `client_config.toml`. Задаёт адрес/порт сервера (значение по
//! умолчанию в меню), имя/пароль аккаунта и список серверов для лобби.
//!
//! Поиск (native): `--config <path>` → рабочий каталог → рядом с бинарём.
//! Wasm: вшитый `client_config.toml` + переопределение `?server=host:port` и
//! `#cert_digest` из URL страницы.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

const FILE_NAME: &str = "client_config.toml";

/// Digest self-signed сертификата WebTransport (dev). На wasm вшивается из
/// `certificates/digest.txt` при сборке; можно переопределить hash-фрагментом URL.
#[cfg(target_arch = "wasm32")]
const DEFAULT_CERT_DIGEST: &str = include_str!("../../certificates/digest.txt");

/// Один сервер из списка лобби. Адрес — строка `ip:port` (тот же порт, что и
/// игровой; на нём же отвечает мета по TCP).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerEntry {
    /// Имя по умолчанию (если мета сервера недоступна, покажем его).
    #[serde(default)]
    pub name: Option<String>,
    /// Адрес `ip:port`.
    pub address: String,
}

#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct ClientConfig {
    /// IP/хост сервера, к которому подключаемся по умолчанию (поле ручного ввода).
    pub ip: String,
    /// UDP-порт сервера (нативный клиент).
    pub port: u16,
    /// WebTransport-порт (браузерный клиент). По умолчанию UDP+1.
    #[serde(default = "default_webtransport_port")]
    pub webtransport_port: u16,
    /// Hex-digest self-signed TLS для WebTransport (dev). Пустая строка — доверять
    /// только настоящим сертификатам (prod с Let's Encrypt).
    #[serde(default)]
    pub cert_digest: String,
    /// Имя аккаунта (логин) для авторизации на сервере и строки в таблице очков.
    #[serde(default = "default_name")]
    pub name: String,
    /// Пароль аккаунта. При первом входе под этим именем он регистрируется,
    /// при повторном — сверяется. Пустой пароль допустим (имя без защиты).
    #[serde(default)]
    pub password: String,
    /// Список серверов для мини-лобби. Каждый проверяется на активность.
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
}

fn default_webtransport_port() -> u16 {
    6001
}

fn default_name() -> String {
    "Player".to_string()
}

/// Первая непустая строка без `#`-комментария (формат `certificates/digest.txt`).
pub fn parse_digest_file(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .to_string()
}

/// Короткий случайный хвост к имени в стиле Discord (`Player#a3f9`).
fn random_suffix() -> String {
    let nanos = crate::platform::now_nanos();
    let pid_mix = crate::platform::random_u64();
    let mut x = nanos ^ pid_mix.rotate_left(32) ^ nanos.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{:04x}", x & 0xffff)
}

/// Сгенерированное имя для случая «конфига рядом нет» — `Player#xxxx`.
fn generated_name() -> String {
    format!("Player#{}", random_suffix())
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: 6000,
            webtransport_port: default_webtransport_port(),
            cert_digest: String::new(),
            name: default_name(),
            password: String::new(),
            servers: vec![ServerEntry {
                name: Some("Локальный сервер".to_string()),
                address: "127.0.0.1:6000".to_string(),
            }],
        }
    }
}

impl ClientConfig {
    /// Строка вида `ip:port` для поля адреса в меню (UDP, нативный клиент).
    pub fn address(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }

    /// Адрес WebTransport для браузерного клиента.
    pub fn webtransport_address(&self) -> String {
        format!("{}:{}", self.ip, self.webtransport_port)
    }

    /// Адрес подключения для текущей платформы.
    pub fn connect_address(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            self.webtransport_address()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.address()
        }
    }

    /// Digest TLS для WebTransportClientIo.
    pub fn effective_cert_digest(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            if !self.cert_digest.is_empty() {
                return self.cert_digest.clone();
            }
            parse_digest_file(DEFAULT_CERT_DIGEST)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.cert_digest.clone()
        }
    }

    /// Digest для connect: runtime (wasm HTTP) → конфиг/URL → embedded.
    pub fn connect_cert_digest(&self, runtime: Option<&str>) -> String {
        if let Some(d) = runtime.filter(|s| !s.is_empty()) {
            return d.to_string();
        }
        self.effective_cert_digest()
    }
}

/// Явный путь из аргумента `--config <path>` или `--config=<path>`, если задан.
#[cfg(not(target_arch = "wasm32"))]
fn cli_config_path() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--config" {
            return args.next().map(PathBuf::from);
        } else if let Some(rest) = a.strip_prefix("--config=") {
            return Some(PathBuf::from(rest));
        }
    }
    None
}

/// Кандидаты на авто-поиск конфига (native).
#[cfg(not(target_arch = "wasm32"))]
fn auto_candidates() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(FILE_NAME)];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(FILE_NAME);
            if !v.contains(&p) {
                v.push(p);
            }
        }
    }
    v
}

fn parse_or_panic(text: &str, path: &Path) -> ClientConfig {
    match toml::from_str::<ClientConfig>(text) {
        Ok(cfg) => {
            info!("⚙ config loaded: {} (name = {})", path.display(), cfg.name);
            cfg
        }
        Err(e) => {
            eprintln!(
                "[config] FATAL: не удалось разобрать {}:\n{e}\n\
                 Почините TOML (или удалите файл — тогда сгенерируется случайное имя).",
                path.display()
            );
            panic!("битый конфиг {}: {e}", path.display());
        }
    }
}

/// Конфиг по умолчанию со случайным именем (без файла на диске).
fn generated_default() -> ClientConfig {
    ClientConfig {
        name: generated_name(),
        ..ClientConfig::default()
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_url_overrides(cfg: &mut ClientConfig) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(search) = window.location().search() {
        if let Some(server) = parse_query_param(&search, "server") {
            if let Some((host, port)) = split_host_port(&server) {
                cfg.ip = host;
                cfg.webtransport_port = port;
                info!("⚙ wasm: server из URL → {}", cfg.webtransport_address());
            }
        }
    }
    if let Ok(hash) = window.location().hash() {
        let digest = hash.trim_start_matches('#').trim();
        if digest.len() > 10 {
            cfg.cert_digest = digest.to_string();
            info!("⚙ wasm: cert_digest из URL hash");
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_query_param(search: &str, key: &str) -> Option<String> {
    let q = search.trim_start_matches('?');
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next()?;
        if k == key {
            return it.next().map(|v| v.to_string());
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn split_host_port(addr: &str) -> Option<(String, u16)> {
    if let Some((host, port)) = addr.rsplit_once(':') {
        let port: u16 = port.parse().ok()?;
        return Some((host.to_string(), port));
    }
    Some((addr.to_string(), default_webtransport_port()))
}

/// Загружает конфиг клиента.
pub fn load_or_create() -> ClientConfig {
    #[cfg(target_arch = "wasm32")]
    {
        let mut cfg = parse_or_panic(
            include_str!("../../client_config.toml"),
            Path::new("embedded/client_config.toml"),
        );
        apply_url_overrides(&mut cfg);
        if cfg.name == default_name() || cfg.name.is_empty() {
            cfg.name = generated_name();
        }
        return cfg;
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(path) = cli_config_path() {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("[config] FATAL: --config {}: {e}", path.display());
                panic!("не удалось прочитать --config {}: {e}", path.display());
            });
            return parse_or_panic(&text, &path);
        }
        for path in auto_candidates() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                return parse_or_panic(&text, &path);
            }
        }
        let cfg = generated_default();
        eprintln!(
            "[config] конфиг не найден; имя сгенерировано: {} (файл не создаётся)",
            cfg.name
        );
        cfg
    }
}
