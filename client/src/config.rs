//! Конфиг клиента в файле РЯДОМ с бинарём (`client_config.toml`). Задаёт адрес и
//! порт сервера, которые подставляются в меню как значение по умолчанию. Если
//! файла нет — создаётся с дефолтами (127.0.0.1:6000). Битый файл → дефолты.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE_NAME: &str = "client_config.toml";

#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct ClientConfig {
    /// IP/хост сервера, к которому подключаемся по умолчанию.
    pub ip: String,
    /// UDP-порт сервера (QUIC).
    pub port: u16,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: 6000,
        }
    }
}

impl ClientConfig {
    /// Строка вида `ip:port` для поля адреса в меню.
    pub fn address(&self) -> String {
        format!("{}:{}", self.ip, self.port)
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
pub fn load_or_create() -> ClientConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<ClientConfig>(&text) {
            Ok(cfg) => {
                info!("⚙ config loaded: {}", path.display());
                cfg
            }
            Err(e) => {
                warn!(
                    "ошибка чтения {}: {e}; беру значения по умолчанию",
                    path.display()
                );
                ClientConfig::default()
            }
        },
        Err(_) => {
            let cfg = ClientConfig::default();
            let header = "# Конфиг клиента CS2D.\n\
                # ip/port сервера — подставятся в меню как адрес по умолчанию.\n";
            match toml::to_string_pretty(&cfg) {
                Ok(body) => {
                    if let Err(e) = std::fs::write(&path, format!("{header}{body}")) {
                        warn!("не удалось создать {}: {e}", path.display());
                    } else {
                        info!("⚙ создан конфиг по умолчанию: {}", path.display());
                    }
                }
                Err(e) => warn!("сериализация конфига не удалась: {e}"),
            }
            cfg
        }
    }
}
