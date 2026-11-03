# Instalar Latex Frog 0.9.2

Editor de proyectos LaTeX grandes para Windows: compila solo lo que cambió (partes, cache de figuras), muestra el PDF al lado y salta del código al PDF y al revés.

## Qué hace falta antes

- **Windows 10 u 11 de 64 bits.** El instalador usa WebView2, que ya viene con Windows 11 y con Edge; si falta, el instalador lo baja (necesita internet esa vez).
- **MiKTeX** con **latexmk**. latexmk es un script de Perl, así que también hace falta **Perl** (Strawberry Perl o ActiveState). MiKTeX instala sus paquetes faltantes a pedido; conviene dejarlo en «instalar siempre» para que no frene una compilación con un cuadro de diálogo.
- **biber**, si el proyecto usa biblatex (el libro lo usa). Viene con MiKTeX: la primera vez que se lo pide lo instala.
- **LaTeX 2021-06 o posterior** (lo que trae cualquier MiKTeX actual). La app lo comprueba y lo avisa.

No hace falta tener VS Code ni SumatraPDF: el editor y el visor del PDF son los de la app.

## Instalar

1. Ejecutar `Latex Frog_0.9.2_x64-setup.exe`. Se instala **para el usuario actual**, sin permisos de administrador, y crea un acceso directo en el menú Inicio.
2. **Windows va a mostrar «Windows protegió su PC» (SmartScreen).** El instalador no está firmado. Apretar «Más información» y después «Ejecutar de todas formas».
3. Abrir Latex Frog.

## La primera vez con un proyecto

1. Apretar **Abrir carpeta…** y elegir la carpeta del proyecto (la que tiene el `.tex` principal).
2. Si el proyecto **no tiene `latexviewer.json`**, se abre sola la pantalla **Proyecto**: detecta el documento principal, las partes (capítulos), las macros de compilación reducida, el cache de figuras, los perfiles y las salidas, y lo explica. No escribe nada hasta que se aprieta **Guardar** y se confirma el cambio.
3. El **Doctor** corre al abrir y avisa lo que falte o esté mal (MiKTeX, latexmk, Perl, biber, una variable sin guardia). Se vuelve a ver desde el panel inferior.
4. **Ctrl+Enter** compila con el perfil elegido; **Ctrl+Alt+T** abre el menú de compilación (perfiles, variables, exportar, publicar, cocinar figuras).

La app guarda lo suyo en la carpeta `.lv/` del proyecto (si el proyecto es un repositorio git, la agrega al `.gitignore`). No abrir el mismo proyecto en dos instancias a la vez: la segunda avisa y no compila.

## Usarla con tu copia del libro

1. **Actualizá MiKTeX antes de instalar** (Consola de MiKTeX › Actualizaciones). Todo se probó con MiKTeX 26.5 y latexmk 4.88; el `README.md` del libro dice 25.4.
2. **Abrí la carpeta del libro** (la que tiene `Funciones de Aproximación.tex`). Como no tiene `latexviewer.json`, se abre la pantalla Proyecto con la configuración ya armada: documento principal, los capítulos, la compilación reducida, el cache de figuras, los perfiles (`parcial`, `rapido`, `completo` y `entrega`), la carpeta de publicación y la de exportación. En la prueba salió igual a la que se escribió a mano, sin tocar nada. Revisala y confirmá.
3. **Si querés elegir el pie desde la app** (Ctrl+Alt+T › Pie de página), cambiá estas dos líneas de `structure.tex` (buscá `\def\OpcionPie`, está donde se arma `\PieDeAutor`). Sin el cambio la app compila igual, pero el doctor avisa que `OpcionPie` no tiene guardia y el menú no la puede cambiar.

   Antes:

   ```latex
   \def\OpcionPie{completo}
   \InputIfFileExists{opciones}{}{}
   ```

   Después:

   ```latex
   \ifdefined\OpcionPie\else\def\OpcionPie{completo}\fi
   \ifdefined\lvPerfil\else\InputIfFileExists{opciones}{}{}\fi
   ```

   Con el cambio, `Menu.bat` sigue eligiendo el pie como hoy (lee `opciones.tex`) y la app lo elige con su menú.
4. **`Menu.bat` y VS Code siguen funcionando mientras tanto.** No los uses a la vez que la app sobre la misma carpeta (escriben los mismos auxiliares). Los PDF de la app quedan en `.lv/build/<perfil>/`, no en `PDF/`; lo que se entrega sale de **Publicar** (menú de compilación), que copia el PDF del perfil `entrega` a `PDF/`.

## Desinstalar

Configuración › Aplicaciones › Latex Frog › Desinstalar. Los proyectos no se tocan (la carpeta `.lv/` de cada uno se puede borrar a mano).
