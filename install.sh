#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs kait2en-touchbar-plus for the current user. No sudo. Idempotent.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/kait2en-touchbar-plus/target}"
share="$HOME/.local/share/touchbar-plus"
ext="$HOME/.local/share/gnome-shell/extensions/touchbar-plus@kait2en"
dropin="$HOME/.config/systemd/user/kait2en-touchbar.service.d"
uuid="touchbar-plus@kait2en"
bin="$HOME/.local/bin/kait2en-touchbar-plus"

# Requisitos: KaiT2en debe estar instalado (unidad, grupo, udev y binario base).
missing=0
need() { # need <descripcion> <comando...>
  local what=$1; shift
  if ! "$@" >/dev/null 2>&1; then
    echo "Falta $what; instale KaiT2en primero (/usr/local/src/KaiT2en-Fedora)" >&2
    missing=1
  fi
}
need "cargo" command -v cargo
need "la unidad kait2en-touchbar.service" systemctl --user cat kait2en-touchbar.service
need "/usr/local/bin/kait2en-touchbar" test -x /usr/local/bin/kait2en-touchbar
need "libinput (libinput-devel)" pkg-config --exists libinput
[ "$missing" -eq 0 ] || exit 1
for c in gtk-launch magick notify-send; do
  command -v "$c" >/dev/null 2>&1 || echo "Aviso: falta $c (algunas funciones no andarán)" >&2
done

( cd "$here/daemon" && cargo build --release --locked )
install -Dm755 "$CARGO_TARGET_DIR/release/kait2en-touchbar-plus" "$bin"

mkdir -p "$dropin"
cat > "$dropin/plus.conf" <<'CONF'
[Service]
ExecStart=
ExecStart=%h/.local/bin/kait2en-touchbar-plus
RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6
CONF

# Extensión: detectar si cambió antes de copiar.
ext_changed=0
for f in metadata.json extension.js; do
  cmp -s "$here/extension/$f" "$ext/$f" || ext_changed=1
done
mkdir -p "$ext"
install -m644 "$here/extension/metadata.json" "$here/extension/extension.js" "$ext/"

mkdir -p "$share/icons/lucide" "$share/qml" "$HOME/.local/share/applications"
install -m755 "$here/app/touchbar-plus-settings" "$share/touchbar-plus-settings"
install -m644 "$here"/app/qml/*.qml "$here/app/qml/qmldir" "$share/qml/"
install -m644 "$here/daemon/config-template.toml" "$here/daemon/layout-template.toml" "$share/"
install -m644 "$here"/daemon/icons/lucide/*.svg "$here/daemon/icons/lucide/LICENSE" "$share/icons/lucide/"
install -m644 "$here/LICENSE" "$share/LICENSE"
# Quitar lo que ya no existe en el repo.
for f in "$share"/qml/* "$share"/icons/lucide/*; do
  [ -e "$f" ] || continue
  case "$f" in
    "$share"/qml/*) src="$here/app/qml/${f##*/}" ;;
    *) src="$here/daemon/icons/lucide/${f##*/}" ;;
  esac
  [ -e "$src" ] || rm -f "$f"
done
cat > "$HOME/.local/share/applications/org.kait2en.TouchbarPlus.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Touch Bar Plus
Comment=Configura la Touch Bar
Exec="$share/touchbar-plus-settings"
Icon=input-touchpad-symbolic
Categories=Settings;HardwareSettings;
Terminal=false
DESKTOP

python3 -c 'import PyQt6.QtQml, PyQt6.QtQuick, PyQt6.QtGui, PyQt6.QtSvg' 2>/dev/null \
  || echo "Aviso: falta python3-pyqt6 para la app de ajustes (sudo dnf install python3-pyqt6)"

# El binario ya está instalado: validar la configuración antes de reiniciar.
if ! "$bin" --check-config; then
  echo "Configuración inválida: no se reinició el servicio. Corríjala y ejecute de nuevo ./install.sh" >&2
  exit 1
fi
systemctl --user daemon-reload
systemctl --user restart kait2en-touchbar

if ! gnome-extensions list 2>/dev/null | grep -qx "$uuid"; then
  echo "El shell aún no conoce la extensión: cierre sesión e inicie otra vez, luego: gnome-extensions enable $uuid"
elif ! gnome-extensions list --enabled 2>/dev/null | grep -qx "$uuid"; then
  gnome-extensions enable "$uuid" || true
  echo "Extensión habilitada. Si no responde, cierre sesión para cargarla."
elif [ "$ext_changed" -eq 1 ]; then
  echo "Cierre sesión para cargar la extensión nueva."
fi
echo "kait2en-touchbar-plus instalado."
