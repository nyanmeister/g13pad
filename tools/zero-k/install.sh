#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs the Zero-K alert widget and the relay that moves its lines into g13pad's feed.
#   tools/zero-k/install.sh [--zerok DIR] [--g13map PATH] [--uninstall]
# The widget lands in DIR/LuaUI/Widgets; the relay runs as a user service watching
# DIR/g13pad_alerts.txt (LuaUI can only write inside the game's own folder).
set -eu
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
zerok=${ZEROK_DIR:-$HOME/.local/share/Steam/steamapps/common/Zero-K}
g13map=$(command -v g13map || true)
uninstall=0
while [ $# -gt 0 ]; do
  case $1 in
    --zerok) zerok=$2; shift 2 ;;
    --g13map) g13map=$2; shift 2 ;;
    --uninstall) uninstall=1; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/g13map-zerok-relay.service"
widget="$zerok/LuaUI/Widgets/g13pad_alerts.lua"
if [ "$uninstall" = 1 ]; then
  systemctl --user disable --now g13map-zerok-relay.service 2>/dev/null || true
  rm -f "$unit" "$widget" "$zerok/g13pad_alerts.txt"
  systemctl --user daemon-reload
  echo "removed the widget, the relay service and the feed file"
  exit 0
fi
[ -d "$zerok/LuaUI" ] || { echo "no LuaUI folder under $zerok; is that the Zero-K install?" >&2; exit 1; }
[ -n "$g13map" ] && [ -x "$g13map" ] || { echo "g13map not found; pass --g13map PATH" >&2; exit 1; }
mkdir -p "$zerok/LuaUI/Widgets" "$(dirname "$unit")"
install -m 644 "$here/g13pad_alerts.lua" "$widget"
cat > "$unit" <<UNIT
[Unit]
Description=g13pad: relay Zero-K alert lines into the G13 feed

[Service]
ExecStart=$g13map alert relay $zerok/g13pad_alerts.txt
Restart=on-failure
Nice=10

[Install]
WantedBy=default.target
UNIT
systemctl --user daemon-reload
systemctl --user enable --now g13map-zerok-relay.service
echo "widget: $widget"
echo "relay:  $(systemctl --user is-active g13map-zerok-relay.service) ($g13map alert relay $zerok/g13pad_alerts.txt)"
echo "profile: g13map profile alert zero-k on   (then focus the game: WM_CLASS 'spring')"
