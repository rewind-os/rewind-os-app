<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/io.github.r_dvl.rewind_os.png" width="128" height="128" alt="Icono de Rewind OS: un mapache grabado sobre una loseta verde azulado">
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

- **Resumen**: unidades (SSD/HDD), memoria y los archivos grandes del propio Windows
  (`hiberfil.sys`, `pagefile.sys`).
- **Analizador de disco**: escanea una unidad o carpeta en paralelo; lista de carpetas, treemap,
  archivos más grandes y tamaño por tipo de archivo. Cualquier elemento se puede mostrar en el
  gestor de archivos o mover a la papelera.
- **Limpieza de basura**: temporales, descargas de actualizaciones, informes de bloqueos, cachés
  de navegadores y apps, cachés de gestores de paquetes, instaladores antiguos en Descargas y la
  papelera. Cada categoría indica lo segura que es y si necesita permisos de administrador.
- **Restos de programas**: carpetas en Program Files, ProgramData y AppData a las que no hace
  referencia ningún programa instalado, servicio, programa en ejecución ni entrada de inicio. Van
  a la papelera.
- **Programas instalados**, con búsqueda y su propio desinstalador.
- **Programas de inicio**: activar y desactivar, con los registros del propio sistema.
- **Mantenimiento**: las herramientas del propio sistema (almacén de componentes e hibernación en
  Windows, el journal en Linux).

No se borra nada sin preguntar. Los archivos que un programa tiene abiertos se omiten y se
informa de ellos. La app arranca como usuario normal y, en Windows, ofrece reiniciarse como
administrador cuando una tarea lo necesita.

## Sistemas

| Función | Windows | Linux | macOS |
|---|:-:|:-:|:-:|
| Resumen, analizador de disco | ✓ | ✓ | ✓ |
| Limpieza, papelera | ✓ | ✓ | ✓ |
| Programas de inicio | ✓ | ✓ (XDG autostart) | — |
| Restos de programas, programas instalados | ✓ | — | — |
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
