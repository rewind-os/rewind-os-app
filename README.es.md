<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/io.github.rewind_os.rewind_os_app.png" width="128" height="128" alt="Icono de Rewind OS: un mapache grabado sobre una loseta verde azulado">
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
  archivos más grandes y tamaño por tipo de archivo. Cualquier elemento se puede mostrar en el
  gestor de archivos o mover a la papelera.
- **Archivos duplicados**: archivos con exactamente el mismo contenido (tamaño, después el primer
  bloque y después un hash BLAKE3 completo). Los enlaces duros no cuentan como copias y los
  archivos que solo están en la nube no se descargan. Siempre queda una copia de cada archivo; las
  demás van a la papelera.
- **Limpieza de basura**: temporales, descargas de actualizaciones, informes de bloqueos, cachés
  de navegadores y apps, cachés de gestores de paquetes, instaladores antiguos en Descargas y la
  papelera. Cada categoría indica lo segura que es y si necesita permisos de administrador.
- **Restos de programas**: carpetas en Program Files, ProgramData y AppData a las que no hace
  referencia ningún programa instalado, servicio, programa en ejecución ni entrada de inicio. Van
  a la papelera. También informa de las claves de registro de programas eliminados, sin borrarlas nunca.
- **Programas instalados**, con sus iconos, búsqueda y su propio desinstalador.
- **Programas de inicio**: activar y desactivar, con los registros del propio sistema, incluidas
  las tareas programadas que añaden los programas (las del propio Windows nunca se muestran).
- **Aligerar Windows**: desactiva anuncios, sugerencias, rastreo y extras que no usas. Una lista
  cerrada de ajustes documentados; no se desinstala nada y cada uno se puede deshacer.
- **Mantenimiento**: las herramientas del propio sistema (almacén de componentes e hibernación en
  Windows, el journal en Linux).

No se borra nada sin preguntar. Los archivos que un programa tiene abiertos se omiten y se
informa de ellos. La app arranca como usuario normal y, en Windows, ofrece reiniciarse como
administrador cuando una tarea lo necesita.

## Sistemas

| Función | Windows | Linux | macOS |
|---|:-:|:-:|:-:|
| Resumen, analizador de disco, archivos duplicados | ✓ | ✓ | ✓ |
| Limpieza, papelera | ✓ | ✓ | ✓ |
| Programas de inicio | ✓ | ✓ (XDG autostart) | — |
| Recomendaciones | ✓ | ✓ | ✓ |
| Programas instalados | ✓ | ✓ (lista: APT, RPM, pacman, Flatpak, Snap) | — |
| Restos de programas, aligerar Windows | ✓ | — | — |
| Mantenimiento | almacén de componentes, hibernación | journal | — |

Una herramienta que aún no está disponible lo dice, en la app y en la CLI; nunca finge
funcionar. Las versiones para macOS se comprueban en CI pero aún no se publican.

## Descargar

Hay versiones listas en la [página de releases](../../releases); puedes comprobarlas con
`SHA256SUMS.txt`. No se instala nada.

- **Windows** x64 y ARM64: descomprime `rewind-os-<versión>-windows-<arq>-portable.zip` y
  ejecuta `rewind-os.exe`.
- **Linux** x64 y ARM64: descomprime `rewind-os-<versión>-linux-<arq>.tar.gz` y ejecuta
  `bin/rewind-os`. Copia su carpeta `share/` en `~/.local/share` para tener el lanzador y el icono.

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

## Licencia

MIT OR Apache-2.0, a tu elección. Consulta [NOTICE](NOTICE).
