#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Re-applies the fork after a KaiT2en update: the override, the binary and the service.
set -euo pipefail
dropin="$HOME/.config/systemd/user/kait2en-touchbar.service.d"
bin="$HOME/.local/bin/kait2en-touchbar-plus"
log() { echo "touchbar-plus-repair: $*"; }

if ! systemctl --user cat kait2en-touchbar.service >/dev/null 2>&1; then
  log "la unidad de KaiT2en aún no existe (actualización en curso); el vigilante lo reintenta al reinstalarse"
  exit 0
fi
if [ ! -x "$bin" ]; then
  log "falta $bin: ejecute ./install.sh en el repo del fork"
  exit 1
fi
if ! "$bin" --check-config >/dev/null 2>&1; then
  log "configuración inválida: no se reinicia (corrija y ejecute --check-config)"
  exit 1
fi
if ! grep -qs "ExecStart=%h/.local/bin/kait2en-touchbar-plus" "$dropin/plus.conf"; then
  mkdir -p "$dropin"
  cat > "$dropin/plus.conf" <<'CONF'
[Service]
ExecStart=
ExecStart=%h/.local/bin/kait2en-touchbar-plus
RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6
CONF
  log "override restaurado"
  systemctl --user daemon-reload
fi
# The daemon is the fork only if systemd resolved the override.
if ! systemctl --user show kait2en-touchbar.service -p ExecStart --value | grep -q kait2en-touchbar-plus; then
  systemctl --user daemon-reload
fi
if ! systemctl --user is-active --quiet kait2en-touchbar.service; then
  systemctl --user restart kait2en-touchbar.service
  log "servicio reiniciado"
fi
