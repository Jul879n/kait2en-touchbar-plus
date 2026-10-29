#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
B="${1:-}"
if [ -z "$B" ] || [ ! -d "$B" ]; then echo "uso: $0 <carpeta-de-backup> (no existe: ${B:-vacío})" >&2; exit 1; fi
B=$(cd "$B" && pwd)
rm -f ~/.config/systemd/user/kait2en-touchbar.service.d/plus.conf
rmdir ~/.config/systemd/user/kait2en-touchbar.service.d 2>/dev/null || true
systemctl --user daemon-reload
gnome-extensions disable touchbar-plus@kait2en || true
src=$(sed -n '/^## sources/{n;p}' "$B/estado.txt")
[ -n "$src" ] && gsettings set org.gnome.desktop.input-sources sources "$src"
[ -d "$B$HOME/.config/kait2en-touchbar" ] && cp -a "$B$HOME/.config/kait2en-touchbar/." ~/.config/kait2en-touchbar/
[ -f "$B$HOME/.local/state/kait2en-touchbar/state.toml" ] && cp -a "$B$HOME/.local/state/kait2en-touchbar/state.toml" ~/.local/state/kait2en-touchbar/state.toml
systemctl --user restart kait2en-touchbar
echo "restaurado desde $B"
