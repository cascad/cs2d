//! Лёгкий «query»-протокол для мини-лобби: клиент перед подключением узнаёт,
//! жив ли сервер и сколько на нём игроков, не открывая полноценное игровое
//! QUIC-соединение.
//!
//! Транспорт — обычный TCP на ТОМ ЖЕ номере порта, что и игровой QUIC (UDP):
//! UDP и TCP-сокеты на одном порту независимы, поэтому пользователю не нужен
//! отдельный порт в конфиге. Сервер на любое входящее TCP-соединение пишет
//! одну короткую текстовую мету и закрывает соединение.
//!
//! Формат намеренно минимальный (только мета, без тяжёлых данных уровня —
//! чтобы массовый опрос из лобби был дешёвым) и не требует доп. зависимостей:
//! строки вида `ключ=значение`, разделённые `\n`.
//!
//! ```text
//! CS2D_META 1
//! name=My Server
//! players=3
//! max=32
//! ```

/// Магическая строка + версия в первой строке ответа.
pub const MAGIC: &str = "CS2D_META";
/// Версия формата меты.
pub const VERSION: u32 = 1;

/// Минимальная мета сервера, отдаваемая по query-запросу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMeta {
    /// Человекочитаемое имя сервера (из конфига сервера).
    pub name: String,
    /// Сколько игроков сейчас на сервере.
    pub players: u32,
    /// Лимит игроков (0 — лимит не задан/неизвестен).
    pub max_players: u32,
}

impl ServerMeta {
    /// Сериализация в текстовый ответ (см. формат в шапке модуля).
    pub fn encode(&self) -> String {
        // имя чистим от переводов строк, чтобы не сломать построчный формат
        let name = self.name.replace(['\n', '\r'], " ");
        format!(
            "{MAGIC} {VERSION}\nname={name}\nplayers={}\nmax={}\n",
            self.players, self.max_players
        )
    }

    /// Разбор ответа сервера. Возвращает `None`, если это не наш протокол.
    pub fn parse(text: &str) -> Option<ServerMeta> {
        let mut lines = text.lines();
        let header = lines.next()?;
        let mut hdr = header.split_whitespace();
        if hdr.next()? != MAGIC {
            return None;
        }
        // версию читаем, но пока не делаем жёсткой проверки на равенство —
        // достаточно, чтобы это было число (forward-compat по minor-полям).
        let _ver: u32 = hdr.next()?.parse().ok()?;

        let mut name = String::new();
        let mut players = 0u32;
        let mut max_players = 0u32;
        for line in lines {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "name" => name = v.trim().to_string(),
                "players" => players = v.trim().parse().unwrap_or(0),
                "max" => max_players = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        Some(ServerMeta {
            name,
            players,
            max_players,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let m = ServerMeta {
            name: "Test Server".into(),
            players: 5,
            max_players: 32,
        };
        let parsed = ServerMeta::parse(&m.encode()).unwrap();
        assert_eq!(m, parsed);
    }

    #[test]
    fn rejects_foreign() {
        assert!(ServerMeta::parse("HTTP/1.1 200 OK\r\n\r\n").is_none());
        assert!(ServerMeta::parse("").is_none());
    }

    #[test]
    fn name_with_newlines_is_sanitized() {
        let m = ServerMeta {
            name: "bad\nname".into(),
            players: 0,
            max_players: 0,
        };
        let parsed = ServerMeta::parse(&m.encode()).unwrap();
        assert_eq!(parsed.name, "bad name");
    }
}
