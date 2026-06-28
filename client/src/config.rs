//! Конфиг клиента `client_config.toml`. Задаёт адрес/порт сервера (значение по
//! умолчанию в меню), имя/пароль аккаунта и список серверов для лобби.
//!
//! Поиск: `--config <path>` (если задан) → рабочий каталог → рядом с бинарём.
//! - Файл найден, но TOML битый → клиент ПАДАЕТ с логом (лучше упасть, чем молча
//!   зайти под случайным именем — это и приводило к «опять рандомный ник»).
//! - `--config` указан, но файла нет/не читается → тоже падаем (его задали явно).
//! - Конфига нигде нет → НЕ ломаемся: дефолты (127.0.0.1:6000) со случайным именем
//!   `Player#xxxx`; на диск ничего не пишем, поэтому при перезапуске имя новое.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "client_config.toml";

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
    /// UDP-порт сервера (QUIC).
    pub port: u16,
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

fn default_name() -> String {
    "Player".to_string()
}

/// Короткий случайный хвост к имени в стиле Discord (`Player#a3f9`). Без внешних
/// крейтов: мешаем наносекунды и pid, прогоняем через xorshift и берём 4 hex.
/// Этого «кусочка» хватает, чтобы клиенты на одной машине не сливались в один
/// аккаунт, и при каждом перезапуске (без конфига) имя получается новым.
fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut x = nanos ^ pid.rotate_left(32) ^ nanos.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
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

/// Кандидаты на авто-поиск конфига (когда `--config` не задан), по приоритету:
/// 1) рабочий каталог (так его находит `cargo run` из корня проекта);
/// 2) каталог рядом с бинарём (так его находит распакованная сборка).
/// Используется ПЕРВЫЙ существующий.
fn auto_candidates() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(FILE_NAME)]; // cwd/client_config.toml
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

/// Конфиг по умолчанию, но со СГЕНЕРИРОВАННЫМ именем (`Player#xxxx`). Используем
/// ТОЛЬКО когда конфига нигде нет: файл при этом НЕ создаём, поэтому при каждом
/// перезапуске клиента имя будет новым.
fn generated_default() -> ClientConfig {
    ClientConfig {
        name: generated_name(),
        ..ClientConfig::default()
    }
}

/// Парсит текст конфига или ЛОМАЕТ клиент с понятным логом. Битый TOML — это
/// ошибка пользователя (а не повод молча подменить имя на случайное): лучше упасть
/// громко, чем зайти в игру под чужим/рандомным ником.
fn parse_or_panic(text: &str, path: &Path) -> ClientConfig {
    match toml::from_str::<ClientConfig>(text) {
        Ok(cfg) => {
            info!("⚙ config loaded: {} (name = {})", path.display(), cfg.name);
            cfg
        }
        Err(e) => {
            // load_or_create вызывается ДО инициализации логгера Bevy — пишем в
            // stderr напрямую (panic туда же), чтобы сообщение точно было видно.
            eprintln!(
                "[config] FATAL: не удалось разобрать {}:\n{e}\n\
                 Почините TOML (или удалите файл — тогда сгенерируется случайное имя).",
                path.display()
            );
            panic!("битый конфиг {}: {e}", path.display());
        }
    }
}

/// Загружает конфиг клиента:
/// - `--config <path>`: файл ОБЯЗАН существовать и парситься, иначе — паника;
/// - иначе ищем `client_config.toml` в рабочем каталоге и рядом с бинарём; первый
///   существующий парсим (битый → паника с логом);
/// - если конфига нигде нет — клиент НЕ ломается: дефолты со случайным именем
///   `Player#xxxx` (на диск ничего не пишем, поэтому при перезапуске имя новое).
pub fn load_or_create() -> ClientConfig {
    // 1) Явный путь — обязателен и не прощает ошибок (его задали намеренно).
    if let Some(path) = cli_config_path() {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("[config] FATAL: --config {}: {e}", path.display());
            panic!("не удалось прочитать --config {}: {e}", path.display());
        });
        return parse_or_panic(&text, &path);
    }
    // 2) Авто-поиск: первый существующий кандидат. Битый — паника (см. parse_or_panic).
    for path in auto_candidates() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_or_panic(&text, &path);
        }
    }
    // 3) Конфига нигде нет — это НЕ ошибка: дефолты со случайным именем.
    let cfg = generated_default();
    eprintln!(
        "[config] конфиг не найден (ни в рабочем каталоге, ни рядом с бинарём); \
         имя сгенерировано: {} (файл не создаётся — при перезапуске будет новое)",
        cfg.name
    );
    cfg
}
