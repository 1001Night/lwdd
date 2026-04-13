#!/bin/bash
set -e

if [ "$EUID" -ne 0 ]; then
    echo "Запусти с sudo"
    exit 1
fi

INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/lddns"
SERVICE_DIR="/etc/systemd/system"

echo "Установка LDDNS..."

if [ -f "./client" ]; then
    cp ./client "$INSTALL_DIR/lddns-client"
    chmod +x "$INSTALL_DIR/lddns-client"
    echo "✓ Клиент установлен в $INSTALL_DIR/lddns-client"
fi

if [ -f "./server" ]; then
    cp ./server "$INSTALL_DIR/lddns-server"
    chmod +x "$INSTALL_DIR/lddns-server"
    echo "✓ Сервер установлен в $INSTALL_DIR/lddns-server"
fi

mkdir -p "$CONFIG_DIR"

cat > "$CONFIG_DIR/client.conf" <<EOF
SERVER=auto
HOSTNAME=$(hostname)
SUBNET=auto
ENABLED=false
EOF

echo "✓ Конфиг создан в $CONFIG_DIR/client.conf"

cat > "$SERVICE_DIR/lddns-client.service" <<EOF
[Unit]
Description=LDDNS Client
After=network.target

[Service]
Type=simple
EnvironmentFile=$CONFIG_DIR/client.conf
ExecStart=$INSTALL_DIR/lddns-client --hostname \${HOSTNAME} --server \${SERVER}
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

cat > "$SERVICE_DIR/lddns-server.service" <<EOF
[Unit]
Description=LDDNS Server
After=network.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/lddns-server --port 53 --domain local
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

echo "✓ Systemd сервисы созданы"

cat > "$INSTALL_DIR/lddns" <<'EOF'
#!/bin/bash

CONFIG_FILE="/etc/lddns/client.conf"
RESOLV_CONF="/etc/resolv.conf"
BACKUP_RESOLV="/etc/lddns/resolv.conf.backup"

source "$CONFIG_FILE" 2>/dev/null || true

case "$1" in
    enable)
        if [ "$EUID" -ne 0 ]; then
            echo "Требуется sudo"
            exit 1
        fi

        SERVER_IP="${2:-auto}"

        if [ ! -f "$BACKUP_RESOLV" ]; then
            cp "$RESOLV_CONF" "$BACKUP_RESOLV"
        fi

        if [ "$SERVER_IP" = "auto" ]; then
            SERVER_IP=$(ip route get 1.1.1.1 | grep -oP 'src \K\S+')
        fi

        echo "nameserver $SERVER_IP" > "$RESOLV_CONF"
        chattr +i "$RESOLV_CONF"

        sed -i "s/^ENABLED=.*/ENABLED=true/" "$CONFIG_FILE"
        sed -i "s/^SERVER=.*/SERVER=$SERVER_IP/" "$CONFIG_FILE"

        systemctl enable --now lddns-client

        echo "✓ LDDNS включен (DNS: $SERVER_IP)"
        ;;

    disable)
        if [ "$EUID" -ne 0 ]; then
            echo "Требуется sudo"
            exit 1
        fi

        systemctl disable --now lddns-client

        chattr -i "$RESOLV_CONF" 2>/dev/null || true

        if [ -f "$BACKUP_RESOLV" ]; then
            cp "$BACKUP_RESOLV" "$RESOLV_CONF"
        else
            echo "nameserver 8.8.8.8" > "$RESOLV_CONF"
            echo "nameserver 1.1.1.1" >> "$RESOLV_CONF"
        fi

        sed -i "s/^ENABLED=.*/ENABLED=false/" "$CONFIG_FILE"

        echo "✓ LDDNS отключен (DNS восстановлен)"
        ;;

    status)
        if systemctl is-active --quiet lddns-client; then
            echo "LDDNS: активен"
            echo "DNS сервер: $(grep nameserver "$RESOLV_CONF" | head -1 | awk '{print $2}')"
        else
            echo "LDDNS: неактивен"
        fi
        ;;

    config)
        if [ -n "$2" ] && [ -n "$3" ]; then
            sed -i "s/^$2=.*/$2=$3/" "$CONFIG_FILE"
            echo "✓ $2=$3"
        else
            cat "$CONFIG_FILE"
        fi
        ;;

    *)
        echo "Использование: lddns {enable|disable|status|config [KEY VALUE]}"
        echo ""
        echo "  enable [SERVER_IP]  - Включить LDDNS (авто или указать IP сервера)"
        echo "  disable             - Отключить LDDNS"
        echo "  status              - Показать статус"
        echo "  config [KEY VALUE]  - Показать/изменить конфиг"
        exit 1
        ;;
esac
EOF

chmod +x "$INSTALL_DIR/lddns"
echo "✓ Команда lddns установлена"

systemctl daemon-reload

echo ""
echo "Установка завершена!"
echo ""
echo "Команды:"
echo "  sudo lddns enable          - Включить LDDNS"
echo "  sudo lddns disable         - Отключить LDDNS"
echo "  lddns status               - Статус"
echo "  lddns config               - Показать конфиг"
echo ""
