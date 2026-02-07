#!/bin/bash
#
# Prism Server - Linux Installation Script
# Installs Prism as a systemd user service
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}"
echo "╔═══════════════════════════════════════════╗"
echo "║         Prism Server Installer            ║"
echo "║         Hybrid Search Engine              ║"
echo "╚═══════════════════════════════════════════╝"
echo -e "${NC}"

# Detect script location
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Configuration
PRISM_HOME="${XDG_DATA_HOME:-$HOME/.local/share}/prism"
BINARY_NAME="prism-server"
SERVICE_NAME="prism"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo -e "${YELLOW}Configuration:${NC}"
echo "  Repo:        $REPO_ROOT"
echo "  Prism Home:  $PRISM_HOME"
echo "  Service:     $SERVICE_NAME (systemd user)"
echo ""

# Check if binary exists
BINARY_PATH="$REPO_ROOT/target/release/$BINARY_NAME"
if [[ ! -f "$BINARY_PATH" ]]; then
    echo -e "${YELLOW}Binary not found. Building...${NC}"

    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: Rust/Cargo not installed.${NC}"
        echo "Install from: https://rustup.rs"
        exit 1
    fi

    cd "$REPO_ROOT"
    echo "Building prism-server (release mode)..."
    cargo build -p prism-server --release

    if [[ ! -f "$BINARY_PATH" ]]; then
        echo -e "${RED}Build failed!${NC}"
        exit 1
    fi
    echo -e "${GREEN}Build complete!${NC}"
    echo ""
fi

# Create directories
echo -e "${BLUE}Creating directories...${NC}"
mkdir -p "$PRISM_HOME"/{data,schemas,logs,cache}
mkdir -p "$SYSTEMD_USER_DIR"

# Create default config if not exists
CONFIG_PATH="$PRISM_HOME/prism.toml"
if [[ ! -f "$CONFIG_PATH" ]]; then
    echo -e "${BLUE}Creating default configuration...${NC}"
    cat > "$CONFIG_PATH" << EOF
# Prism Server Configuration

[server]
# bind_addr is set via CLI args

[server.cors]
enabled = true
origins = ["http://localhost:5173", "http://127.0.0.1:5173"]

[server.tls]
enabled = false

[storage]
data_dir = "$PRISM_HOME/data"

[observability]
log_level = "info"
log_format = "text"
metrics_enabled = true

[security]
enabled = false

[security.audit]
enabled = false

[embedding]
enabled = true

[embedding.provider]
type = "ollama"
url = "http://localhost:11434"
model = "nomic-embed-text"
EOF
fi

# Stop existing service if running
if systemctl --user is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo -e "${YELLOW}Stopping existing service...${NC}"
    systemctl --user stop "$SERVICE_NAME"
fi

# Generate systemd unit from template
echo -e "${BLUE}Installing systemd service...${NC}"
TEMPLATE="$SCRIPT_DIR/prism.service.template"
SERVICE_PATH="$SYSTEMD_USER_DIR/${SERVICE_NAME}.service"

if [[ -f "$TEMPLATE" ]]; then
    sed -e "s|{{BINARY_PATH}}|$BINARY_PATH|g" \
        -e "s|{{CONFIG_PATH}}|$CONFIG_PATH|g" \
        -e "s|{{SCHEMAS_DIR}}|$PRISM_HOME/schemas|g" \
        -e "s|{{DATA_DIR}}|$PRISM_HOME/data|g" \
        -e "s|{{PRISM_HOME}}|$PRISM_HOME|g" \
        "$TEMPLATE" > "$SERVICE_PATH"
else
    cat > "$SERVICE_PATH" << EOF
[Unit]
Description=Prism Hybrid Search Server
After=network-online.target

[Service]
Type=simple
ExecStart=$BINARY_PATH --config $CONFIG_PATH --host 127.0.0.1 --port 3080 --schemas-dir $PRISM_HOME/schemas --data-dir $PRISM_HOME/data
WorkingDirectory=$PRISM_HOME
Environment=RUST_LOG=info,prism=debug
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF
fi

# Reload and enable
systemctl --user daemon-reload
systemctl --user enable "$SERVICE_NAME"
systemctl --user start "$SERVICE_NAME"

# Wait for startup
sleep 2

# Check status
if systemctl --user is-active --quiet "$SERVICE_NAME"; then
    echo ""
    echo -e "${GREEN}╔═══════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║         Installation Complete!            ║${NC}"
    echo -e "${GREEN}╚═══════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "  ${BLUE}Server:${NC}  http://127.0.0.1:3080"
    echo -e "  ${BLUE}Health:${NC}  curl http://127.0.0.1:3080/health"
    echo -e "  ${BLUE}Logs:${NC}    journalctl --user -u $SERVICE_NAME -f"
    echo ""
    echo -e "  ${YELLOW}Commands:${NC}"
    echo "    Status:  systemctl --user status $SERVICE_NAME"
    echo "    Restart: systemctl --user restart $SERVICE_NAME"
    echo "    Stop:    systemctl --user stop $SERVICE_NAME"
    echo ""

    # Enable lingering so service runs without login
    if command -v loginctl &> /dev/null; then
        echo -e "  ${YELLOW}Tip:${NC} Run 'loginctl enable-linger' to keep service running after logout"
    fi
else
    echo -e "${RED}Service failed to start. Check logs:${NC}"
    echo "  journalctl --user -u $SERVICE_NAME -n 50"
    exit 1
fi
