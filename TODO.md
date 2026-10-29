# TODO kait2en-touchbar-plus

Restaurar: `./restore.sh backup/2026-10-28_1851`

- [x] Paso 1/16: Respaldo, TODO del proyecto y fork compilable
- [x] Paso 2/16: Redirigir el servicio al fork (reversible)
- [x] Paso 3/16: Configuración propia con opciones nuevas y `--check-config`
- [x] Paso 4/16: Modo siempre encendido + oscuro solo al bloquear
- [x] Paso 5/16: Protección OLED (atenuar + pixel shift)
- [x] Paso 6/16: Motor de layout configurable (`layout.toml`)
- [x] Paso 7/16: Widgets locales
- [x] Paso 8/16: Clima (Open-Meteo + GeoClue o manual)
- [x] Paso 9/16: Próximo evento del calendario
- [x] Paso 10/16: Interruptores con estado y captura de pantalla
- [x] Paso 11/16: Música: canción actual, carátula, barra arrastrable y visualizador
- [x] Paso 12/16: Sliders al mantener presionado volumen o brillo
- [x] Paso 13/16: Extensión GNOME `touchbar-plus@kait2en`
- [x] Paso 14/16: Perfiles por app y filas transitorias
- [x] Paso 15/16: App de configuración «Touch Bar Plus»
- [x] Paso 16/16: `install.sh` / `uninstall.sh` y cierre del TODO
## Revisión posterior (plan de 14 pasos) - [x] Cuadrícula de 24 columnas, reproducción por ancho, selector de apps, diálogos AT-SPI, Touch ID de 6 estilos, tarjeta de notificaciones, perfil Terminal con Ghostty y apagado de la barra con la pantalla: ver `README.md`. - [ ] Pendiente en vivo: ver la barra apagarse con la pantalla y encenderse 2 min con un toque.

## Pendiente de verificar en vivo (código listo, sin prueba física)
- [x] Bloqueo y desbloqueo verificados en vivo (2026-10-28).
- Atenuación a brillo 1 tras 2 min sin tocar trackpad ni teclado; pixel shift cada 3 min (log `shift=`).
- Extensión: cerrar sesión, iniciar, `gnome-extensions enable touchbar-plus@kait2en`; luego perfiles por app, notificaciones, ventanas, emojis y sugerencias.
- Sugerencias: configurar idiomas en `ibus-setup-typing-booster` (es_CL + en_US).
- Próximo evento con un evento real de calendario; slider al mantener volumen/brillo; arrastre de la canción actual.
- [x] Idiomas: textos es/en (barra y app) y diccionarios de sugerencias es_CL,en_US.
- [x] Botón único `controls` (volumen, brillo, luz del teclado) con sliders; configurable en la app con vista previa.
- [x] Notificaciones por monitor del bus de sesión (`notify.rs`); la extensión también las reenvía.
- [ ] Pendiente en vivo: tocar `controls` y deslizar cada slider; ver un aviso real (`notify-send`) en la barra.
- [x] App nueva (maqueta B): escenario, perfiles como pestañas, paleta, inspector, apariencia y restaurar.
- [x] `icon_color = "full"` y ítems `app` con ícono real (código y `--check-config` verificados).
- [ ] Pendiente en vivo: ver el color completo y un ícono de app en la barra física.
