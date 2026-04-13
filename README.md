# LDDNS - Lightweight Dynamic DNS

Клиент-серверная система динамического DNS на Rust для Linux и Windows.

## Возможности

- Автоматическая регистрация хостов в локальной сети
- DNS сервер с fallback на публичные DNS (AdGuard, Cloudflare)
- Heartbeat каждые 30 секунд
- Автоочистка устаревших записей
- Установочные скрипты для Linux и Windows
- Systemd/Windows Service интеграция
- Команды для управления DNS

## Установка

### Linux

```bash
# Скачай бинарники
wget https://github.com/1001Night/lwdd/releases/latest/download/lddns-client-linux
wget https://github.com/1001Night/lwdd/releases/latest/download/lddns-server-linux

# Установи
chmod +x install.sh
sudo ./install.sh
```

### Windows

```powershell
# Скачай бинарники и install.ps1
# Запусти PowerShell от администратора
.\install.ps1
```

## Использование

### Включить LDDNS

```bash
# Linux
sudo lddns enable

# Windows
lddns enable
```

### Отключить LDDNS

```bash
# Linux
sudo lddns disable

# Windows
lddns disable
```

### Статус

```bash
lddns status
```

### Конфигурация

```bash
# Показать конфиг
lddns config

# Изменить hostname
lddns config HOSTNAME mypc
```

## DNS Fallback

Если hostname не найден в локальной базе, запрос проксируется на:
1. 94.140.14.15 (AdGuard DNS)
2. 94.140.14.16 (AdGuard DNS)
3. 1.1.1.1 (Cloudflare)
4. 1.0.0.1 (Cloudflare)

## Ручной запуск

### Сервер

```bash
sudo lddns-server --port 53 --domain local --subnet 192.168.1
```

### Клиент

```bash
lddns-client --hostname mypc --server 192.168.1.1
```

## Автодеплой

При push в `main` GitHub Actions автоматически деплоит сервер на `user1001@192.144.14.240`.

Требуется добавить SSH ключ в GitHub Secrets:
- `SSH_PRIVATE_KEY` - приватный SSH ключ для доступа к серверу

## Разработка

```bash
cargo build --release
cargo build --release --target x86_64-pc-windows-gnu
```

## Лицензия

MIT
