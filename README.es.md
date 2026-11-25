<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/io.github.rewind_os.rewind_os_app.png" width="128" height="128" alt="Icono de Rewind OS: una cinta de casete rebobinada, con el carrete izquierdo lleno de cinta azul, sobre una loseta azul marino">
</p>

<h1 align="center">Rewind OS</h1>

<p align="center">
  <a href="README.md">English</a>
</p>

<p align="center">
  <b>Mira qué ocupa tu disco y devuelve tu equipo a un estado limpio.</b><br>
  Una app nativa, pequeña y rápida, escrita solo en Rust. Primero Windows; Linux y macOS en camino.
</p>

Elimina lo que no necesitas: archivos temporales, cachés, restos de programas que desinstalaste,
archivos grandes olvidados y programas que arrancan con el sistema sin motivo.

- **Inicio**: recomendaciones para este equipo (una unidad casi llena, basura que conviene
  limpiar, demasiados programas de inicio, duplicados en Descargas, restos de programas
  eliminados…), cada una con un botón que lo hace; unidades (SSD/HDD), memoria y los archivos grandes del propio Windows
  (`hiberfil.sys`, `pagefile.sys`).
- **Analizador de disco**: escanea una unidad o carpeta en paralelo; lista de carpetas, treemap,
  archivos más grandes y tamaño por tipo de archivo. Como administrador en Windows, lee de una
  vez la tabla maestra de archivos de una unidad NTFS, en segundos. «Qué ha cambiado» señala las
  carpetas que más han crecido o menguado desde el último escaneo de la misma carpeta (un resumen
  breve de tamaños de carpetas se guarda en este equipo, en la carpeta de datos de la app). Cualquier elemento se puede mostrar en el
  gestor de archivos o mover a la papelera.
- **Archivos duplicados**: archivos con exactamente el mismo contenido (tamaño, después el primer
  bloque y después un hash BLAKE3 completo). Los enlaces duros no cuentan como copias y los
  archivos que solo están en la nube no se descargan. Siempre queda una copia de cada archivo; las
  demás van a la papelera.
- **Carpetas de compilación**: `node_modules`, `target` de Rust, entornos virtuales de Python,
  salidas de Gradle, Maven, .NET, Flutter y CMake, y cachés de frameworks web de tus proyectos. Una
  carpeta solo cuenta junto al archivo que demuestra qué es (`package.json` con su lockfile,
  `Cargo.toml` con la etiqueta de caché de Cargo), nunca dentro de carpetas ocultas, datos de apps
  o programas. Se borran del todo, porque cada proyecto las vuelve a crear (`npm install`,
  `cargo build`).
- **Limpieza de basura**: temporales, descargas de actualizaciones, informes de bloqueos, cachés
  de navegadores y apps, cachés de gestores de paquetes, instaladores antiguos en Descargas y la
  papelera. Cada categoría indica lo segura que es y si necesita permisos de administrador.
- **Restos de programas**: carpetas en Program Files, ProgramData y AppData a las que no hace
  referencia ningún programa instalado, servicio, programa en ejecución ni entrada de inicio. Van
  a la papelera. También informa de las claves de registro de programas eliminados, sin borrarlas nunca.
- **Programas instalados**, con sus iconos, búsqueda y su propio desinstalador.
- **Programas de inicio**: activar y desactivar, con los registros del propio sistema, incluidas
  las tareas programadas y los servicios que añaden los programas (los del propio Windows nunca
  se muestran; los servicios pasan a arrancar cuando se necesitan, nunca se deshabilitan).
- **Velocidad de arranque**: cuánto tarda en arrancar el equipo, sus últimos arranques y qué los
  ralentizó (el registro de rendimiento del propio Windows, como administrador; `systemd-analyze`
  en Linux), con un interruptor para el programa de inicio que causa un retraso.
- **Aligerar Windows**: desactiva anuncios, sugerencias, rastreo y extras que no usas. Una lista
  cerrada de ajustes documentados, cada uno se puede deshacer, y quitar de forma opcional apps
  preinstaladas de la Store (Noticias, Consejos, Clipchamp…) de tu cuenta; la Store puede volver
  a instalarlas.
- **Mantenimiento**: las herramientas del propio sistema (almacén de componentes e hibernación en
  Windows, el journal en Linux).

No se borra nada sin preguntar. Los archivos que un programa tiene abiertos se omiten y se
informa de ellos. La app arranca como usuario normal y, en Windows, ofrece reiniciarse como
administrador cuando una tarea lo necesita.

## Sistemas

| Función | Windows | Linux | macOS |
|---|:-:|:-:|:-:|
| Resumen, analizador de disco, archivos duplicados, carpetas de compilación | ✓ | ✓ | ✓ |
| Limpieza, papelera | ✓ | ✓ | ✓ |
| Programas de inicio | ✓ | ✓ (XDG autostart) | ✓ (LaunchAgents) |
| Velocidad de arranque | ✓ (como administrador) | ✓ (systemd, último arranque) | — |
| Recomendaciones | ✓ | ✓ | ✓ |
| Programas instalados | ✓ | ✓ (APT, RPM, pacman, Flatpak, Snap) | ✓ (apps, a la Papelera) |
| Restos de programas | ✓ | ✓ (`~/.config`, `~/.local/share`) | ✓ (`~/Library`) |
| Aligerar Windows | ✓ | — | — |
| Mantenimiento | almacén de componentes, hibernación | journal | — |

Una herramienta que aún no está disponible lo dice, en la app y en la CLI; nunca finge
funcionar. Las versiones para macOS se comprueban en CI pero aún no se publican.

## Descargar

Hay versiones listas en la [página de releases](../../releases); puedes comprobarlas con
`SHA256SUMS.txt`, firmado con la clave de [packaging/minisign.pub](packaging/minisign.pub)
(ver [packaging/README.md](packaging/README.md#release-signature)).

- **Windows** x64 y ARM64: ejecuta `rewind-os-<versión>-windows-<arq>.msi` para instalarlo (menú
  Inicio, se desinstala desde Configuración), o descomprime
  `rewind-os-<versión>-windows-<arq>-portable.zip` y ejecuta `rewind-os.exe` sin instalar nada.
- **Linux** x64 y ARM64 (glibc 2.35 o posterior: Debian 12, Ubuntu 22.04, Fedora 36 y
  posteriores): descomprime `rewind-os-<versión>-linux-<arq>.tar.gz` y ejecuta
  `bin/rewind-os`. Copia su carpeta `share/` en `~/.local/share` para tener el lanzador y el icono.
  O instala el paquete: `sudo apt install ./rewind-os_<versión>_amd64.deb` (Debian, Ubuntu) o
  `sudo dnf install ./rewind-os-<versión>-1.x86_64.rpm` (Fedora, openSUSE); `arm64` y `aarch64`
  para ARM.
  O instala el Flatpak: `flatpak install --user ./rewind-os-<versión>-linux-<arq>.flatpak` (necesita
  acceso al sistema de archivos para ver el disco y ejecuta las herramientas del sistema fuera del
  sandbox).

### Versiones nuevas

Una vez al día la app pregunta a GitHub si hay una versión nueva y, si la hay, lo dice al pie de
la navegación. La petición solo lleva el nombre y la versión de la app. Se desactiva desde el
número de versión, junto al nombre de la app, o con `rewind-os-cli update off`;
`rewind-os-cli update check` lo comprueba al momento.

El MSI y las versiones portables se actualizan con un clic («Actualizar a …», o
`rewind-os-cli update install --yes`): la app descarga la versión nueva, solo la instala si su
suma está en el `SHA256SUMS.txt` firmado de la release, y vuelve a abrirse. Los .deb, .rpm y el
Flatpak los actualiza el gestor de paquetes del sistema o Flatpak.

## Compilar

Con [Rust](https://rustup.rs) (en Windows, también las Build Tools de C++ de Visual Studio):

```sh
cargo build --release
target/release/rewind-os        # app de escritorio
target/release/rewind-os-cli    # línea de comandos
```

## Línea de comandos

Todas las funciones de la app son también comandos; consulta el [README en inglés](README.md#command-line).
Añade `--lang es` para los mensajes en español.

`rewind-os-cli mcp` ofrece las mismas herramientas a un asistente con el Model Context Protocol.
Por defecto solo las que no cambian nada; `--automation-read-root`, `--automation-write-root` y
`--allow-system-changes` abren más, de una en una ([detalles](README.md#assistants-mcp)).

## Licencia

MIT OR Apache-2.0, a tu elección. Consulta [NOTICE](NOTICE).
