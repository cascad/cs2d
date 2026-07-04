# Харденинг VPS (Ubuntu)

Порядок строгий: root и пароли отключаются ТОЛЬКО после проверки входа по ключу
новым пользователем во втором окне терминала — иначе легко отрезать себе доступ.

## 1. Новый пользователь с sudo (на VPS под root)

```bash
adduser deploy          # пароль задать — он нужен для sudo
usermod -aG sudo deploy
```

В группу `docker` пользователя НЕ добавляем: членство в ней эквивалентно root
(контейнер с примонтированным `/` = полный доступ). Управляем докером через
`sudo docker ...`.

## 2. SSH-ключ с локальной машины

Windows (PowerShell; ключ создаётся один раз):

```powershell
ssh-keygen -t ed25519    # Enter-Enter, если ключа ещё нет
type $env:USERPROFILE\.ssh\id_ed25519.pub | ssh deploy@<IP> "mkdir -p ~/.ssh && cat >> ~/.ssh/authorized_keys && chmod 700 ~/.ssh && chmod 600 ~/.ssh/authorized_keys"
```

## 3. ПРОВЕРКА (не пропускать!)

Во ВТОРОМ окне: `ssh deploy@<IP>` — должно пустить БЕЗ пароля, и `sudo -v`
работает. Старую сессию root не закрывать, пока шаг 4 не проверен.

## 4. Отключить root-вход и пароли

Ubuntu раскладывает конфиги в `/etc/ssh/sshd_config.d/`, и sshd берёт ПЕРВОЕ
встреченное значение каждого параметра, а включаются файлы по алфавиту —
поэтому наш файл должен сортироваться РАНЬШЕ облачного `50-cloud-init.conf`
(в нём бывает `PasswordAuthentication yes`):

```bash
sudo tee /etc/ssh/sshd_config.d/00-hardening.conf >/dev/null <<'EOF'
PermitRootLogin no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
MaxAuthTries 3
EOF
sudo sshd -t && sudo systemctl restart ssh
```

Проверить в новом окне: `ssh deploy@<IP>` пускает; `ssh root@<IP>` и вход по
паролю — отказ (`Permission denied (publickey)`).

## 5. fail2ban (баны за перебор SSH)

```bash
sudo apt update && sudo apt install -y fail2ban
sudo systemctl enable --now fail2ban
sudo fail2ban-client status sshd    # jail активен, счётчики видны
```

## 6. Автообновления безопасности

```bash
sudo apt install -y unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades   # ответить Yes
```

Обновления ядра требуют перезагрузки; можно включить авторебут в тихое время
(`/etc/apt/apt.conf.d/50unattended-upgrades`):

```
Unattended-Upgrade::Automatic-Reboot "true";
Unattended-Upgrade::Automatic-Reboot-Time "05:00";
```

Игровой сервер это переживает: контейнеры с `restart: unless-stopped`
поднимутся сами, игроки перезайдут по F5.

## 7. (опционально) Нестандартный SSH-порт

Срезает шум ботов в логах (безопасности по сути не добавляет). Порядок
проверен на живом переносе 22 → 2222; текущую сессию не закрывать до
финальной проверки.

```bash
# 1) СНАЧАЛА открыть новый порт (и в файрволе ХОСТЕРА, если он есть, — тоже!)
sudo ufw allow 2222/tcp

# 2) на время перехода — ОБА порта
sudo tee -a /etc/ssh/sshd_config.d/00-hardening.conf >/dev/null <<'EOF'
Port 22
Port 2222
EOF

# 3) ⚠ ГЛАВНАЯ ЛОВУШКА (Ubuntu 22.10+/24.04): ssh по умолчанию
# socket-активирован — порт держит systemd (ssh.socket, ListenStream=22),
# и `Port` из sshd_config МОЛЧА игнорируется. Симптом: в
# `ss -tlnp | grep sshd` рядом со sshd висит ("systemd",pid=1,...) и только :22.
# Переключаемся на классический сервис:
systemctl is-active ssh.socket && {
    sudo systemctl disable --now ssh.socket
    sudo systemctl enable ssh.service
}

sudo sshd -t && sudo systemctl restart ssh
sudo ss -tlnp | grep sshd   # должны быть :22 И :2222, владелец — только sshd

# 4) проверить с клиента: ssh -p 2222 deploy@<IP>  — и только потом:
sudo sed -i '/^Port 22$/d' /etc/ssh/sshd_config.d/00-hardening.conf
sudo sshd -t && sudo systemctl restart ssh
sudo ufw delete allow OpenSSH
```

Хвосты, про которые забывают:

```bash
# fail2ban следит за портом ssh (22) — перевести на новый
sudo tee /etc/fail2ban/jail.d/ssh-port.conf >/dev/null <<'EOF'
[sshd]
port = 2222
EOF
sudo systemctl restart fail2ban
```

И алиас на клиенте (`~/.ssh/config`), чтобы не таскать `-p`:

```
Host cs2d
    HostName <IP>
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
```

Диагностика «не подключается» по типу ошибки: `Connection timed out` —
файрвол (ufw или панель хостера); `Connection refused` — sshd не слушает
порт (см. ловушку с ssh.socket выше); `Permission denied` — сам коннект
в порядке, разбираться с ключами. Рестарт sshd НЕ рвёт живые сессии —
чинить безопасно из уже открытой. Если доступ потерян совсем — веб-консоль
(VNC/serial) в панели хостера.

## Что уже закрыто на уровне проекта

- ufw: открыты только 80/443 (сайт), 6000/6001 (игра), SSH.
- Docker публикует порты в обход ufw — потому наружу торчит ровно то, что
  перечислено в `docker-compose.yml`, ничего лишнего там нет.
- Игровой сервер в контейнере без томов с системными путями; статику отдаёт
  Caddy тоже из контейнера, read-only маунты.
