#!/bin/sh
# ─────────────────────────────────────────────
# 🗑️  Rove Uninstaller (macOS / Linux)
# Usage: curl -fsSL https://get.roveai.co/uninstall.sh | sh
#    or: ./uninstall.sh          (interactive, asks for confirmation)
#    or: ./uninstall.sh --yes    (skip confirmation)
# ─────────────────────────────────────────────
set -e

CHANNEL="stable"
FORCE=false

for arg in "$@"; do
  case "$arg" in
    --yes|-y) FORCE=true ;;
    --channel=*) CHANNEL="${arg#--channel=}" ;;
  esac
done

case "$CHANNEL" in
  stable)
    BINARY="rove"
    PLUGIN_DIR="${HOME}/.rove"
    ;;
  dev|nightly)
    CHANNEL="dev"
    BINARY="rove-dev"
    PLUGIN_DIR="${HOME}/.rove-dev"
    ;;
  *)
    echo "Error: unknown channel '$CHANNEL' (expected 'stable' or 'dev')"
    exit 1
    ;;
esac

# Auto-confirm when piped (stdin is not a terminal)
if [ ! -t 0 ]; then
  FORCE=true
fi

echo ""
printf "  ╭──────────────────────────╮\n"
printf "  │    Rove Uninstaller       │\n"
printf "  ╰──────────────────────────╯\n"
echo ""

# ── Find all installations ──

found=0
locations=""

for dir in /usr/local/bin "${HOME}/.local/bin" "${HOME}/.cargo/bin"; do
  if [ -f "${dir}/${BINARY}" ]; then
    locations="${locations}  ${dir}/${BINARY}\n"
    found=$((found + 1))
  fi
done

if [ "$found" -eq 0 ]; then
  printf "No Rove installation found.\n"
  echo ""
  echo "Checked:"
  echo "  /usr/local/bin/rove"
  echo "  ~/.local/bin/rove"
  echo "  ~/.cargo/bin/rove"
  exit 0
fi

printf "Found Rove in:\n"
printf "$locations"
echo ""

# ── Confirm ──

if [ "$FORCE" = false ]; then
  printf "Remove Rove completely? [y/N] "
  read -r confirm
  if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
    echo "Aborted."
    exit 0
  fi
fi

echo ""

# ── Stop daemon if running ──

if pgrep -x rove >/dev/null 2>&1; then
  printf "  Stopping Rove daemon... "
  pkill -x rove 2>/dev/null || true
  printf "✓\n"
fi

# ── Remove binary ──

for dir in /usr/local/bin "${HOME}/.local/bin" "${HOME}/.cargo/bin"; do
  if [ -f "${dir}/${BINARY}" ]; then
    printf "  Removing ${dir}/${BINARY}... "
    if [ -w "${dir}" ]; then
      rm -f "${dir}/${BINARY}"
    elif command -v sudo >/dev/null 2>&1; then
      sudo rm -f "${dir}/${BINARY}"
    else
      printf "no permission (try with sudo)\n"
      continue
    fi
    printf "✓\n"
  fi
done

# ── Remove config directory ──

CONFIG_DIR="${HOME}/.config/rove"
if [ -d "$CONFIG_DIR" ]; then
  printf "  Removing config %s... " "$CONFIG_DIR"
  rm -rf "$CONFIG_DIR"
  printf "✓\n"
fi

# ── Remove data directory ──

DATA_DIR="${HOME}/.local/share/rove"
if [ -d "$DATA_DIR" ]; then
  printf "  Removing data %s... " "$DATA_DIR"
  rm -rf "$DATA_DIR"
  printf "✓\n"
fi

# ── Remove cache ──

CACHE_DIR="${HOME}/.cache/rove"
if [ -d "$CACHE_DIR" ]; then
  printf "  Removing cache %s... " "$CACHE_DIR"
  rm -rf "$CACHE_DIR"
  printf "✓\n"
fi

# ── Remove macOS-specific paths ──

if [ "$(uname -s)" = "Darwin" ]; then
  MAC_SUPPORT="${HOME}/Library/Application Support/rove"
  if [ -d "$MAC_SUPPORT" ]; then
    printf "  Removing %s... " "$MAC_SUPPORT"
    rm -rf "$MAC_SUPPORT"
    printf "✓\n"
  fi

  MAC_CACHE="${HOME}/Library/Caches/rove"
  if [ -d "$MAC_CACHE" ]; then
    printf "  Removing %s... " "$MAC_CACHE"
    rm -rf "$MAC_CACHE"
    printf "✓\n"
  fi
fi

# ── Remove plugins & WASM cache ──

if [ -d "$PLUGIN_DIR" ]; then
  printf "  Removing plugins %s... " "$PLUGIN_DIR"
  rm -rf "$PLUGIN_DIR"
  printf "✓\n"
fi

# ── Remove launchd/systemd service ──

LAUNCHD_PLIST="${HOME}/Library/LaunchAgents/co.roveai.rove.plist"
if [ -f "$LAUNCHD_PLIST" ]; then
  printf "  Unloading launchd service... "
  launchctl unload "$LAUNCHD_PLIST" 2>/dev/null || true
  rm -f "$LAUNCHD_PLIST"
  printf "✓\n"
fi

SYSTEMD_SERVICE="${HOME}/.config/systemd/user/rove.service"
if [ -f "$SYSTEMD_SERVICE" ]; then
  printf "  Disabling systemd service... "
  systemctl --user disable rove 2>/dev/null || true
  systemctl --user stop rove 2>/dev/null || true
  rm -f "$SYSTEMD_SERVICE"
  systemctl --user daemon-reload 2>/dev/null || true
  printf "✓\n"
fi

echo ""
printf "Rove has been completely uninstalled.\n"
echo ""
