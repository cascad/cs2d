//! Конфиг сервера в файле РЯДОМ с бинарём (`server_config.toml`). Задаёт адрес и
//! порт прослушивания. Если файла нет — создаётся с дефолтами (0.0.0.0:6000),
//! чтобы его было легко найти и поправить. Битый файл → дефолты + предупреждение.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

const FILE_NAME: &str = "server_config.toml";

#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
    /// IP интерфейса для прослушивания. `0.0.0.0` — все интерфейсы (LAN/интернет),
    /// `127.0.0.1` — только локально.
    pub ip: String,
    /// UDP-порт (QUIC). На этом же номере порта поднимается TCP-листенер меты
    /// для мини-лобби клиента (см. `net::start_meta_endpoint`).
    pub port: u16,
    /// Человекочитаемое имя сервера — показывается в списке серверов клиента.
    #[serde(default = "default_name")]
    pub name: String,
    /// Лимит игроков (для отображения в лобби). `0` — лимит не задаётся.
    #[serde(default)]
    pub max_players: u32,
}

fn default_name() -> String {
    "CS2D Server".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ip: "0.0.0.0".to_string(),
            port: 6000,
            name: default_name(),
            max_players: 0,
        }
    }
}

impl ServerConfig {
    /// Разобранный IP (при ошибке парса — 0.0.0.0).
    pub fn ip_addr(&self) -> IpAddr {
        self.ip
            .parse()
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    }
}

/// Явный путь из аргумента `--config <path>` или `--config=<path>`, если задан.
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

/// Путь к конфигу: сперва `--config`, затем рядом с бинарём, иначе — рабочий каталог.
fn config_path() -> PathBuf {
    if let Some(p) = cli_config_path() {
        return p;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join(FILE_NAME);
        }
    }
    PathBuf::from(FILE_NAME)
}

/// Читает конфиг рядом с бинарём; если файла нет — создаёт с дефолтами.
pub fn load_or_create() -> ServerConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<ServerConfig>(&text) {
            Ok(cfg) => {
                println!("⚙ config loaded: {}", path.display());
                cfg
            }
            Err(e) => {
                eprintln!(
                    "⚠ ошибка чтения {}: {e}; беру значения по умолчанию",
                    path.display()
                );
                ServerConfig::default()
            }
        },
        Err(_) => {
            let cfg = ServerConfig::default();
            let header = "# Конфиг сервера CS2D.\n\
                # ip = \"0.0.0.0\"  — слушать на всех интерфейсах (LAN/интернет)\n\
                # ip = \"127.0.0.1\" — только локально\n\
                # port — UDP-порт (QUIC); на этом же порту отвечает мета для лобби (TCP)\n\
                # name — имя сервера в списке серверов клиента\n\
                # max_players — лимит игроков для отображения (0 — без лимита)\n";
            match toml::to_string_pretty(&cfg) {
                Ok(body) => {
                    if let Err(e) = std::fs::write(&path, format!("{header}{body}")) {
                        eprintln!("⚠ не удалось создать {}: {e}", path.display());
                    } else {
                        println!("⚙ создан конфиг по умолчанию: {}", path.display());
                    }
                }
                Err(e) => eprintln!("⚠ сериализация конфига не удалась: {e}"),
            }
            cfg
        }
    }
}
