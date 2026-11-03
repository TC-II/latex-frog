@echo off
rem Abre Latex Frog en modo desarrollo con el libro cargado.
rem Los cambios de la interfaz se ven solos; para cerrarla, cerrar la ventana de la app o esta consola.
title Latex Frog (desarrollo)
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "LV_OPEN=C:\_DATA\ITBA\Repositorios GitHub\01-Overleaf\Funciones-de-Aproximaci-n-Libro-TC-\Proyecto"
call npx tauri dev
echo.
echo La app se cerro. Si fue por un error, lo que quedo escrito arriba sirve para encontrarlo.
pause
