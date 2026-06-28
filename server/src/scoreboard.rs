//! Аккаунты и таблица очков — ЧИСТАЯ детерминированная логика (без Bevy/сети),
//! чтобы её было легко покрыть юнит-тестами (регресс-проверка при будущих
//! рефакторингах сети).
//!
//! Модель идентификации: «аккаунт по имени + пароль», живущий в памяти, пока жив
//! сервер. При первом входе имя регистрируется (пароль солится и хешируется), при
//! повторном — пароль сверяется. Статистика (киллы/смерти/убитые непись) копится
//! на аккаунт и переживает реконнекты (новый client_id привязывается к тому же
//! аккаунту). На рестарте сервера всё обнуляется — это осознанный компромисс
//! «пока сервер жив». Дисковую персистентность можно добавить поверх позже.

use std::collections::HashMap;

use protocol::messages::ScoreEntry;

/// Результат попытки авторизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Имя встретилось впервые — аккаунт создан, клиент привязан.
    Registered,
    /// Аккаунт существует, пароль верный — клиент привязан, статистика возобновлена.
    Authenticated,
    /// Пароль не совпал — вход отклонён.
    WrongPassword,
    /// Под этим аккаунтом уже играет другое активное соединение.
    AlreadyOnline,
}

#[derive(Debug, Clone)]
struct Account {
    /// Отображаемое имя (как ввёл игрок, с регистром).
    display: String,
    salt: u64,
    pass_hash: u64,
    kills: u32,
    npc_kills: u32,
    deaths: u32,
}

/// Реестр аккаунтов + кто сейчас онлайн (client_id → ключ аккаунта).
#[derive(Default)]
pub struct AccountBook {
    /// ключ (нормализованное имя) → аккаунт
    by_key: HashMap<String, Account>,
    /// client_id → ключ аккаунта (только активные соединения)
    online: HashMap<u64, String>,
    /// монотонный счётчик для генерации уникальной соли при регистрации
    salt_seq: u64,
}

/// Нормализация имени в ключ: обрезаем пробелы и приводим к нижнему регистру,
/// чтобы "Bob" и "bob " были одним аккаунтом. Пустое имя → "player".
fn key_of(name: &str) -> String {
    let t = name.trim().to_lowercase();
    if t.is_empty() {
        "player".to_string()
    } else {
        t
    }
}

/// FNV-1a (64-bit) от соли и пароля — детерминированный несекретный хеш, чтобы не
/// держать пароль в открытом виде в памяти. Для in-memory защиты имени этого
/// достаточно; криптостойкость можно усилить вместе с дисковой персистентностью.
fn hash_password(salt: u64, password: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
    for &b in password.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl AccountBook {
    /// Авторизация соединения `client_id` под именем `name` с паролем `password`.
    /// Регистрация при первом входе, иначе сверка пароля. При успехе привязывает
    /// `client_id` к аккаунту.
    pub fn authenticate(&mut self, client_id: u64, name: &str, password: &str) -> AuthOutcome {
        let key = key_of(name);

        // Уже играет кто-то другой под этим аккаунтом?
        if self
            .online
            .iter()
            .any(|(&cid, k)| k == &key && cid != client_id)
        {
            return AuthOutcome::AlreadyOnline;
        }

        if let Some(acc) = self.by_key.get(&key) {
            if hash_password(acc.salt, password) != acc.pass_hash {
                return AuthOutcome::WrongPassword;
            }
            self.online.insert(client_id, key);
            AuthOutcome::Authenticated
        } else {
            self.salt_seq = self.salt_seq.wrapping_add(1);
            // соль уникальна на аккаунт (счётчик ⊕ хеш ключа)
            let salt = self.salt_seq ^ hash_password(0, &key);
            let acc = Account {
                display: name.trim().to_string(),
                salt,
                pass_hash: hash_password(salt, password),
                kills: 0,
                npc_kills: 0,
                deaths: 0,
            };
            self.by_key.insert(key.clone(), acc);
            self.online.insert(client_id, key);
            AuthOutcome::Registered
        }
    }

    /// Авторизовано ли соединение (можно ли принимать его игровые сообщения).
    pub fn is_online(&self, client_id: u64) -> bool {
        self.online.contains_key(&client_id)
    }

    /// Отображаемое имя по client_id (для логов/чата).
    pub fn display_name(&self, client_id: u64) -> Option<&str> {
        let key = self.online.get(&client_id)?;
        self.by_key.get(key).map(|a| a.display.as_str())
    }

    /// Снять привязку соединения (на выходе). Аккаунт и его статистика остаются.
    /// Возвращает ключ аккаунта, если он был онлайн.
    pub fn unbind(&mut self, client_id: u64) -> Option<String> {
        self.online.remove(&client_id)
    }

    fn account_mut(&mut self, client_id: u64) -> Option<&mut Account> {
        let key = self.online.get(&client_id)?.clone();
        self.by_key.get_mut(&key)
    }

    /// +1 убийство игроку (по его client_id).
    pub fn add_kill(&mut self, client_id: u64) {
        if let Some(a) = self.account_mut(client_id) {
            a.kills += 1;
        }
    }

    /// +1 убитая непись игроку (по его client_id).
    pub fn add_npc_kill(&mut self, client_id: u64) {
        if let Some(a) = self.account_mut(client_id) {
            a.npc_kills += 1;
        }
    }

    /// +1 смерть игроку (по его client_id).
    pub fn add_death(&mut self, client_id: u64) {
        if let Some(a) = self.account_mut(client_id) {
            a.deaths += 1;
        }
    }

    /// Зафиксировать смерть игрока: жертве +смерть, убийце (если есть и это не сам
    /// игрок) +килл. Самоубийство/смерть без источника — только смерть.
    pub fn record_player_death(&mut self, victim_id: u64, killer: Option<u64>) {
        self.add_death(victim_id);
        if let Some(k) = killer {
            if k != victim_id {
                self.add_kill(k);
            }
        }
    }

    /// Текущая таблица очков (отсортирована: киллы ↓, затем непись ↓, затем имя ↑).
    /// Включает и оффлайн-аккаунты (online=false, id=0) — видно, что статистика
    /// сохраняется и возобновится при реконнекте.
    pub fn snapshot(&self) -> Vec<ScoreEntry> {
        // обратная карта ключ → client_id для онлайн-игроков
        let mut online_id: HashMap<&str, u64> = HashMap::new();
        for (&cid, key) in &self.online {
            online_id.insert(key.as_str(), cid);
        }

        let mut out: Vec<ScoreEntry> = self
            .by_key
            .iter()
            .map(|(key, a)| {
                let id = online_id.get(key.as_str()).copied();
                ScoreEntry {
                    id: id.unwrap_or(0),
                    name: a.display.clone(),
                    kills: a.kills,
                    npc_kills: a.npc_kills,
                    deaths: a.deaths,
                    online: id.is_some(),
                }
            })
            .collect();

        out.sort_by(|x, y| {
            y.kills
                .cmp(&x.kills)
                .then(y.npc_kills.cmp(&x.npc_kills))
                .then(x.name.to_lowercase().cmp(&y.name.to_lowercase()))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_login_registers_and_binds() {
        let mut b = AccountBook::default();
        assert_eq!(b.authenticate(1, "Bob", "pw"), AuthOutcome::Registered);
        assert!(b.is_online(1));
        assert_eq!(b.display_name(1), Some("Bob"));
    }

    #[test]
    fn name_is_case_and_whitespace_insensitive() {
        let mut b = AccountBook::default();
        assert_eq!(b.authenticate(1, "Bob", "pw"), AuthOutcome::Registered);
        b.unbind(1);
        // тот же аккаунт, другой регистр/пробелы, верный пароль
        assert_eq!(b.authenticate(2, " bOB ", "pw"), AuthOutcome::Authenticated);
    }

    #[test]
    fn wrong_password_is_rejected_and_not_bound() {
        let mut b = AccountBook::default();
        b.authenticate(1, "Bob", "right");
        b.unbind(1);
        assert_eq!(b.authenticate(2, "Bob", "wrong"), AuthOutcome::WrongPassword);
        assert!(!b.is_online(2));
    }

    #[test]
    fn double_login_same_account_is_blocked_while_online() {
        let mut b = AccountBook::default();
        b.authenticate(1, "Bob", "pw");
        assert_eq!(b.authenticate(2, "Bob", "pw"), AuthOutcome::AlreadyOnline);
        // после выхода первого — снова можно зайти
        b.unbind(1);
        assert_eq!(b.authenticate(2, "Bob", "pw"), AuthOutcome::Authenticated);
    }

    #[test]
    fn stats_persist_across_reconnect() {
        let mut b = AccountBook::default();
        b.authenticate(1, "Bob", "pw");
        b.add_kill(1);
        b.add_npc_kill(1);
        b.add_death(1);
        b.unbind(1); // ушёл

        // реконнект под НОВЫМ client_id — та же статистика
        assert_eq!(b.authenticate(99, "Bob", "pw"), AuthOutcome::Authenticated);
        let row = b
            .snapshot()
            .into_iter()
            .find(|e| e.name == "Bob")
            .unwrap();
        assert_eq!((row.kills, row.npc_kills, row.deaths), (1, 1, 1));
        assert_eq!(row.id, 99);
        assert!(row.online);
    }

    #[test]
    fn player_death_credits_killer_only_when_distinct() {
        let mut b = AccountBook::default();
        b.authenticate(1, "Killer", "");
        b.authenticate(2, "Victim", "");

        b.record_player_death(2, Some(1));
        let snap = b.snapshot();
        let killer = snap.iter().find(|e| e.name == "Killer").unwrap();
        let victim = snap.iter().find(|e| e.name == "Victim").unwrap();
        assert_eq!(killer.kills, 1);
        assert_eq!(victim.deaths, 1);

        // самоубийство: только смерть, килл себе не идёт
        b.record_player_death(1, Some(1));
        let snap = b.snapshot();
        let killer = snap.iter().find(|e| e.name == "Killer").unwrap();
        assert_eq!(killer.kills, 1);
        assert_eq!(killer.deaths, 1);
    }

    #[test]
    fn disconnect_death_has_no_killer() {
        // Эмуляция «вышел из игры»: жертве +смерть, киллов никому.
        let mut b = AccountBook::default();
        b.authenticate(1, "Bob", "pw");
        b.add_death(1); // сервер так оформляет выход
        b.unbind(1);
        let row = b.snapshot().into_iter().next().unwrap();
        assert_eq!(row.deaths, 1);
        assert_eq!(row.kills, 0);
        assert!(!row.online);
        assert_eq!(row.id, 0);
    }

    #[test]
    fn snapshot_sorted_by_kills_then_npc_then_name() {
        let mut b = AccountBook::default();
        b.authenticate(1, "Alice", "");
        b.authenticate(2, "Bob", "");
        b.authenticate(3, "Carol", "");
        b.add_kill(2); // Bob: 1 килл
        b.add_npc_kill(1); // Alice: 1 непись
        // Carol: ничего
        let snap = b.snapshot();
        assert_eq!(snap[0].name, "Bob"); // больше киллов
        assert_eq!(snap[1].name, "Alice"); // потом по непись
        assert_eq!(snap[2].name, "Carol");
    }

    #[test]
    fn full_session_scenario_matches_server_contract() {
        // Воспроизводим последовательность серверных событий и проверяем, что
        // итоговая таблица соответствует контракту, на который опираются системы.
        let mut b = AccountBook::default();
        b.authenticate(1, "Alice", "a");
        b.authenticate(2, "Bob", "b");

        // Alice убивает скелета и Боба
        b.add_npc_kill(1);
        b.record_player_death(2, Some(1)); // Bob died from Alice

        // Bob реконнектится под новым id, статистика смертей сохранилась
        b.unbind(2);
        assert_eq!(b.authenticate(7, "Bob", "b"), AuthOutcome::Authenticated);

        // Bob кидает гранату себе под ноги (самоубийство): только смерть
        b.record_player_death(7, Some(7));

        // Alice выходит из игры: +смерть, килл никому
        b.add_death(1);
        b.unbind(1);

        let snap = b.snapshot();
        let alice = snap.iter().find(|e| e.name == "Alice").unwrap();
        let bob = snap.iter().find(|e| e.name == "Bob").unwrap();

        assert_eq!((alice.kills, alice.npc_kills, alice.deaths), (1, 1, 1));
        assert!(!alice.online && alice.id == 0);
        assert_eq!((bob.kills, bob.npc_kills, bob.deaths), (0, 0, 2));
        assert!(bob.online && bob.id == 7);
        // сортировка: у Alice больше киллов → она первая
        assert_eq!(snap[0].name, "Alice");
    }

    #[test]
    fn stat_updates_after_unbind_are_ignored() {
        // после unbind соединение не привязано — счёт не меняем по этому id
        let mut b = AccountBook::default();
        b.authenticate(1, "Bob", "pw");
        b.unbind(1);
        b.add_kill(1); // должно быть проигнорировано
        let row = b.snapshot().into_iter().next().unwrap();
        assert_eq!(row.kills, 0);
    }
}
