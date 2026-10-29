#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Returns to KaiT2en's own daemon. Keeps ~/.config/kait2en-touchbar-plus.
set -euo pipefail
systemctl --user disable --now touchbar-plus-repair.path touchbar-plus-repair.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/touchbar-plus-repair.path" "$HOME/.config/systemd/user/touchbar-plus-repair.service"
rm -f "$HOME/.config/systemd/user/kait2en-touchbar.service.d/plus.conf"
rmdir "$HOME/.config/systemd/user/kait2en-touchbar.service.d" 2>/dev/null || true
gnome-extensions disable touchbar-plus@kait2en 2>/dev/null || true
rm -rf "$HOME/.local/share/gnome-shell/extensions/touchbar-plus@kait2en" \
       "$HOME/.local/share/touchbar-plus" \
       "$HOME/.local/share/applications/org.kait2en.TouchbarPlus.desktop" \
       "$HOME/.local/bin/kait2en-touchbar-plus"
systemctl --user daemon-reload
systemctl --user restart kait2en-touchbar
echo "Daemon de KaiT2en restaurado. Para volver también la fuente de entrada y la config: ./restore.sh backup/<fecha>"
