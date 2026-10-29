# kait2en-touchbar-plus

> **Fork no oficial** de [KaiT2en-Fedora](https://github.com/kaiT2en/KaiT2en-Fedora), de uso personal y sin relación con el proyecto original.
> **Aviso sobre IA:** este código fue desarrollado con ayuda de IA (Claude, de Anthropic) y revisado y probado por su autor en su propio equipo (MacBook T2). Úselo bajo su responsabilidad.
> Licencia GPL-3.0-or-later; los íconos son de [Lucide](https://lucide.dev) (ISC, ver `daemon/icons/lucide/LICENSE`).

Fork del daemon `t2-touchbar` de KaiT2en (`daemon/`), una extensión de GNOME Shell (`extension/`) y la app
«Touch Bar Plus» (`app/`). El checkout de `/usr/local/src/KaiT2en-Fedora` no se toca.

- Requisitos: KaiT2en ya instalado (`/usr/local/src/KaiT2en-Fedora`: unidad, grupo, udev, `/usr/local/bin/kait2en-touchbar`), `cargo`, `libinput-devel`, `python3-pyqt6`, ImageMagick (`magick`), `gtk-launch` y `notify-send`. `install.sh` comprueba los primeros y avisa de los demás.
- Instalar: `./install.sh` (sin sudo). Quitar: `./uninstall.sh`. Respaldo previo: `./restore.sh backup/<fecha>`.
- Configuración: `~/.config/kait2en-touchbar-plus/{config,layout,keys}.toml`, o la app «Touch Bar Plus».
- Validar sin reiniciar: `~/.local/bin/kait2en-touchbar-plus --check-config`.

## Actualizar cuando KaiT2en cambie

    diff -ru /usr/local/src/KaiT2en-Fedora/apps/t2-touchbar daemon

Incorporar a mano lo que corresponda. Si el fork deja de abrir el panel, `./uninstall.sh` devuelve el daemon oficial.

## Escritura con sugerencias

Fuente de entrada IBus typing-booster como segunda fuente (Super+Espacio para alternar):
`gsettings set org.gnome.desktop.input-sources sources "[('xkb', 'us+intl'), ('ibus', 'typing-booster')]"`

## Idiomas

`language = "auto" | "es" | "en"` en `config.toml` fija el idioma de los textos de la barra y de la app
(`auto` sigue `$LANG`). Para otro idioma, agregar una columna en `daemon/src/i18n.rs` y en `EN` de la app.
Los idiomas de las sugerencias de escritura se fijan en la app (General) y se guardan en typing-booster.

## Color completo e íconos de app

`icon_color = "full"` en `config.toml` (o en la app, «Apariencia») dibuja cada ícono Lucide con su color
y los ítems `{ type = "key", app = "org.gnome.Nautilus.desktop" }` con el ícono real de la aplicación
(lanzan `gtk-launch`). Los íconos solo SVG se rasterizan con ImageMagick (`magick`) al primer dibujo.

## Cuadrícula de 24 columnas

La barra es una cuadrícula de 24 columnas. Cada ítem de `layout.toml` admite `col` (1 a 24) y `span` (ancho en
columnas). El `span` mínimo es el número de celdas del ítem (p. ej. `playback` 3, `volume` 4). Un `col` que
solape con el ítem anterior se desplaza a continuación de él. No hay migración desde 12 columnas: un `span`
pensado para 12 ahora ocupa la mitad. Un perfil con `items` vacío deja su zona vacía a propósito.

## Reproducción por ancho

«Canción actual» (`now_playing`) agrega botones según su `span`: 10 columnas, pausa; 14, anterior y siguiente;
18, aleatorio y repetir.

## Selector Super/Alt+Tab

Con `app_switcher = true` la barra muestra las apps del selector Super+Tab / Alt+Tab. Requiere la extensión de GNOME.

## Diálogos de apps

La barra puede mostrar los botones de un diálogo de la app enfocada mediante AT-SPI. En este equipo
`toolkit-accessibility` está en `false` y el diálogo de libadwaita funciona igual (verificado). Si una app no
aparece, `gsettings set org.gnome.desktop.interface toolkit-accessibility true` puede ayudar; es un cambio de
GNOME que corresponde decidir al usuario.

## Touch ID

Seis estilos (`touch_id_style` = A a F) y claves `touch_id_*` (colores, textos, animación, velocidad) en `config.toml`.

## Notificaciones

Se muestran en una tarjeta animada. Se leen del bus de sesión y la extensión de GNOME también las reenvía.
Con la pantalla apagada o bloqueada se descartan.

## Pantalla apagada

La barra se apaga cuando la pantalla se apaga o se bloquea. Un toque la enciende por 2 minutos sin otro toque.

## Luz del teclado

`follow_keyboard_backlight = true` atenúa los glifos junto con la luz del teclado (por defecto `false`).

## Perfil Terminal

El perfil «Terminal» de `layout.toml` cubre Ptyxis y Ghostty (`com.mitchellh.ghostty.desktop`).

## Notas de la interfaz D-Bus

- `InsertText` pega el texto y por eso pisa el portapapeles.
- `SelectSuggestion` usa el índice de la página visible.
