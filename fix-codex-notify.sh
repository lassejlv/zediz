#!/usr/bin/env bash
set -euo pipefail

PROJECT_CONFIG=".codex/config.toml"
USER_CONFIG="$HOME/.codex/config.toml"

if [ ! -f "$PROJECT_CONFIG" ]; then
  echo "No project config found at $PROJECT_CONFIG"
  exit 0
fi

mkdir -p "$HOME/.codex"

# Backup both configs, because we're not animals
cp "$PROJECT_CONFIG" "$PROJECT_CONFIG.bak"

if [ -f "$USER_CONFIG" ]; then
  cp "$USER_CONFIG" "$USER_CONFIG.bak"
else
  touch "$USER_CONFIG"
fi

# Extract notify lines from project config
NOTIFY_LINES="$(grep -E '^[[:space:]]*notify[[:space:]]*=' "$PROJECT_CONFIG" || true)"

if [ -z "$NOTIFY_LINES" ]; then
  echo "No notify key found in $PROJECT_CONFIG"
  exit 0
fi

# Remove existing notify from user config to avoid duplicates
tmp_user="$(mktemp)"
grep -Ev '^[[:space:]]*notify[[:space:]]*=' "$USER_CONFIG" > "$tmp_user" || true
mv "$tmp_user" "$USER_CONFIG"

# Add notify to user config
{
  echo ""
  echo "# Moved from project .codex/config.toml"
  echo "$NOTIFY_LINES"
} >> "$USER_CONFIG"

# Remove notify from project config
tmp_project="$(mktemp)"
grep -Ev '^[[:space:]]*notify[[:space:]]*=' "$PROJECT_CONFIG" > "$tmp_project" || true
mv "$tmp_project" "$PROJECT_CONFIG"

echo "Fixed Codex notify config."
echo "Moved:"
echo "$NOTIFY_LINES"
echo ""
echo "Backups:"
echo "  $PROJECT_CONFIG.bak"
echo "  $USER_CONFIG.bak"
