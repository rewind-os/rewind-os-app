# ReWindows

[English](README.md)

Una app nativa para Windows 11, pequeña y rápida, escrita solo en Rust, que te enseña qué ocupa tu
disco y elimina lo que no necesitas: archivos temporales, cachés, restos de programas que
desinstalaste, archivos grandes olvidados y programas que arrancan con Windows sin motivo.

- **Resumen**: unidades (SSD/HDD), memoria y los archivos grandes del propio Windows
  (`hiberfil.sys`, `pagefile.sys`).
- **Analizador de disco**: escanea una unidad o carpeta en paralelo; lista de carpetas, treemap,
  archivos más grandes y tamaño por tipo de archivo. Cualquier elemento se puede mostrar en el
  Explorador o mover a la papelera.
- **Limpieza de basura**: temporales, descargas de Windows Update, volcados de bloqueos, cachés
  de navegadores y apps, cachés de gestores de paquetes, instaladores antiguos en Descargas y la
  papelera. Cada categoría indica lo segura que es y si necesita permisos de administrador.
- **Restos de programas**: carpetas en Program Files, ProgramData y AppData a las que no hace
  referencia ningún programa instalado, servicio, programa en ejecución ni entrada de inicio. Van
  a la papelera.
- **Programas instalados**, con búsqueda y su propio desinstalador.
- **Programas de inicio**: activar y desactivar, con los mismos registros que usa Windows.
- **Mantenimiento**: tamaño real y limpieza del almacén de componentes (WinSxS), hibernación.

No se borra nada sin preguntar. Los archivos que un programa tiene abiertos se omiten y se
informa de ellos. La app arranca como usuario normal y ofrece reiniciarse como administrador
cuando una tarea lo necesita.

## Compilar

En Windows 11 con [Rust](https://rustup.rs) y las Build Tools de C++ de Visual Studio:

```powershell
cargo build --release
.\target\release\rewindows.exe        # app de escritorio
.\target\release\rewindows-cli.exe    # línea de comandos
```

## Línea de comandos

Todas las funciones de la app son también comandos; consulta el [README en inglés](README.md#command-line).
Añade `--lang es` para los mensajes en español.

## Licencia

MIT OR Apache-2.0, a tu elección. Consulta [NOTICE](NOTICE).
