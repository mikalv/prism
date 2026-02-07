#!/bin/bash
#
# Prism Server - macOS Installation Script
# Double-click this file in Finder to install Prism as a launchd service
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}"
echo "╔═══════════════════════════════════════════╗"
echo "║         Prism Server Installer            ║"
echo "║         Hybrid Search Engine              ║"
echo "╚═══════════════════════════════════════════╝"
echo -e "${NC}"

# Detect script location (where the repo is)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Configuration
PRISM_HOME="${HOME}/Library/Application Support/Prism"
BINARY_NAME="prism-server"
SERVICE_LABEL="rs.mux.prism"
PLIST_PATH="${HOME}/Library/LaunchAgents/${SERVICE_LABEL}.plist"

echo -e "${YELLOW}Configuration:${NC}"
echo "  Repo:        $REPO_ROOT"
echo "  Prism Home:  $PRISM_HOME"
echo "  Service:     $SERVICE_LABEL"
echo ""

# Check if binary exists, offer to build
BINARY_PATH="$REPO_ROOT/target/release/$BINARY_NAME"
if [[ ! -f "$BINARY_PATH" ]]; then
    echo -e "${YELLOW}Binary not found. Building...${NC}"
    echo ""

    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: Rust/Cargo not installed.${NC}"
        echo "Install from: https://rustup.rs"
        read -p "Press Enter to exit..."
        exit 1
    fi

    cd "$REPO_ROOT"
    echo "Building prism-server (release mode)..."
    cargo build -p prism-server --release

    if [[ ! -f "$BINARY_PATH" ]]; then
        echo -e "${RED}Build failed!${NC}"
        read -p "Press Enter to exit..."
        exit 1
    fi
    echo -e "${GREEN}Build complete!${NC}"
    echo ""
fi

# Create directories
echo -e "${BLUE}Creating directories...${NC}"
mkdir -p "$PRISM_HOME"/{data,schemas,logs,cache}
mkdir -p "${HOME}/Library/LaunchAgents"

# Create default config if not exists
CONFIG_PATH="$PRISM_HOME/prism.toml"
if [[ ! -f "$CONFIG_PATH" ]]; then
    echo -e "${BLUE}Creating default configuration...${NC}"
    cat > "$CONFIG_PATH" << 'EOF'
# Prism Server Configuration

[server]
# bind_addr is set via CLI args

[server.cors]
enabled = true
origins = ["http://localhost:5173", "http://127.0.0.1:5173"]

[server.tls]
enabled = false

[storage]
data_dir = "{{PRISM_HOME}}/data"

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
    # Replace placeholder
    sed -i '' "s|{{PRISM_HOME}}|$PRISM_HOME|g" "$CONFIG_PATH"
fi

# Stop existing service if running
if launchctl list | grep -q "$SERVICE_LABEL"; then
    echo -e "${YELLOW}Stopping existing service...${NC}"
    launchctl bootout "gui/$(id -u)/$SERVICE_LABEL" 2>/dev/null || true
fi

# Remove old com.prism.server if exists
if launchctl list | grep -q "com.prism.server"; then
    echo -e "${YELLOW}Removing old com.prism.server...${NC}"
    launchctl bootout "gui/$(id -u)/com.prism.server" 2>/dev/null || true
    rm -f "${HOME}/Library/LaunchAgents/com.prism.server.plist"
fi

# Generate plist from template
echo -e "${BLUE}Installing launchd service...${NC}"
TEMPLATE="$SCRIPT_DIR/rs.mux.prism.plist.template"
if [[ -f "$TEMPLATE" ]]; then
    sed -e "s|{{BINARY_PATH}}|$BINARY_PATH|g" \
        -e "s|{{CONFIG_PATH}}|$CONFIG_PATH|g" \
        -e "s|{{SCHEMAS_DIR}}|$PRISM_HOME/schemas|g" \
        -e "s|{{DATA_DIR}}|$PRISM_HOME/data|g" \
        -e "s|{{PRISM_HOME}}|$PRISM_HOME|g" \
        "$TEMPLATE" > "$PLIST_PATH"
else
    # Inline plist if template not found
    cat > "$PLIST_PATH" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$SERVICE_LABEL</string>
    <key>ProgramArguments</key>
    <array>
        <string>$BINARY_PATH</string>
        <string>--config</string>
        <string>$CONFIG_PATH</string>
        <string>--host</string>
        <string>127.0.0.1</string>
        <string>--port</string>
        <string>3080</string>
        <string>--schemas-dir</string>
        <string>$PRISM_HOME/schemas</string>
        <string>--data-dir</string>
        <string>$PRISM_HOME/data</string>
    </array>
    <key>WorkingDirectory</key>
    <string>$PRISM_HOME</string>
    <key>StandardOutPath</key>
    <string>$PRISM_HOME/logs/prism.log</string>
    <key>StandardErrorPath</key>
    <string>$PRISM_HOME/logs/prism.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info,prism=debug</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
EOF
fi

# Load service
echo -e "${BLUE}Starting service...${NC}"
launchctl bootstrap "gui/$(id -u)" "$PLIST_PATH"

# Wait for startup
sleep 2

# Check status
if launchctl list | grep -q "$SERVICE_LABEL"; then
    echo ""
    echo -e "${GREEN}╔═══════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║         Installation Complete!            ║${NC}"
    echo -e "${GREEN}╚═══════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "  ${BLUE}Server:${NC}  http://127.0.0.1:3080"
    echo -e "  ${BLUE}Health:${NC}  curl http://127.0.0.1:3080/health"
    echo -e "  ${BLUE}Logs:${NC}    tail -f '$PRISM_HOME/logs/prism.log'"
    echo ""
    echo -e "  ${YELLOW}Commands:${NC}"
    echo "    Restart: launchctl kickstart -k gui/\$(id -u)/$SERVICE_LABEL"
    echo "    Stop:    launchctl bootout gui/\$(id -u)/$SERVICE_LABEL"
    echo ""
else
    echo -e "${RED}Service failed to start. Check logs:${NC}"
    echo "  tail '$PRISM_HOME/logs/prism.err'"
fi

read -p "Press Enter to close..."
