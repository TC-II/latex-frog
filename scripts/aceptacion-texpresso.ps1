# Smoke de aceptación de la vista previa con TeXpresso (en WSL): abre en la app una copia del libro y corre dentro de la app real
# (LV_SMOKE=texpresso) con TeXpresso de verdad: el interruptor de la barra apagado y sin procesos, prenderlo con un clic, que cada cambio
# tecleado le llegue en milisegundos sin escribir el archivo, un error de TeX en la barra, un cuelgue simulado que se detecta y se
# reinicia solo, que latexmk siga compilando igual, y apagarlo sin dejar procesos. Después, desde afuera: que al salir de la app no
# quede ningún proceso de TeXpresso en la distro, y una segunda corrida (LV_SMOKE_FILE=reabrir) en la que el interruptor guardado
# prendido lanza TeXpresso solo al abrir el proyecto.
#
# Uso:  powershell -NoProfile -File scripts\aceptacion-texpresso.ps1 [-LimiteMin 15] [-Distro Ubuntu]
#
# Necesita la app compilada (npx tauri build --no-bundle), fixtures\libro-tc (scripts\copiar-libro.ps1, con el cache de figuras
# tibio) y WSL2 con TeXpresso, TeX Live y WSLg (la ventana de TeXpresso aparece unos segundos). Trabaja sobre una copia en
# spikes\work\texpresso-smoke\ (ignorada por git). Sale con 0 si todo dio ok.

param(
    [int]$LimiteMin = 15,
    [string]$Distro = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$exe = if ($env:LV_APP_EXE) { $env:LV_APP_EXE } else { Join-Path $repo 'target\release\lv-app.exe' }
$abiertas = @(Get-Process -Name 'lv-app' -ErrorAction SilentlyContinue)
if ($abiertas.Count -gt 0 -and $env:LV_ACEPTAR_APP_ABIERTA -ne '1') {
    Write-Host ("Hay {0} instancia(s) de lv-app.exe abierta(s) (PID {1}). Cerrala(s) y volvé a correr el smoke." -f $abiertas.Count, (($abiertas | ForEach-Object { $_.Id }) -join ', ')) -ForegroundColor Red
    exit 2
}
if (-not (Test-Path $exe)) { throw "Falta $exe. Compilá con: npx tauri build --no-bundle" }
if (-not (Test-Path (Join-Path $repo 'fixtures\libro-tc'))) { throw 'Falta fixtures\libro-tc: corré antes scripts\copiar-libro.ps1.' }

# La distro: la pedida o la que eligiría la app (la por omisión que no sea de Docker). Sólo para contar procesos desde afuera.
if (-not $Distro) {
    $lista = (wsl.exe -l -q | Out-String) -replace "`0", ''
    $Distro = ($lista -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ -and $_ -notlike 'docker-desktop*' } | Select-Object -First 1)
}
if (-not $Distro) { Write-Host 'WSL no tiene una distro de Linux: no se puede probar TeXpresso.' -ForegroundColor Red; exit 1 }
$env:LV_TEXPRESSO_DISTRO = $Distro
function Contar-Procesos { [int]((wsl.exe -d $Distro -e pgrep -fc lv-texpresso.tex | Out-String).Trim()) }
$antes = Contar-Procesos
if ($antes -gt 0) { Write-Host "Ya hay $antes procesos de TeXpresso de la app en $Distro (de otra corrida): matalos con «pkill -f lv-texpresso.tex» en la distro." -ForegroundColor Red; exit 1 }

$base = Join-Path $repo 'spikes\work\texpresso-smoke'
$carpeta = Join-Path $base 'Proyecto'
$config = Join-Path $base 'config'
$salida = Join-Path $base 'resultado.json'
if (Test-Path $base) { Remove-Item $base -Recurse -Force }
New-Item -ItemType Directory -Force $config | Out-Null
& (Join-Path $PSScriptRoot 'copiar-libro.ps1') -Limpio -Origen 'fixtures\libro-tc' -Destino 'spikes\work\texpresso-smoke\Proyecto' | Out-Null
# El interruptor arranca apagado (el state.json de la copia puede venir de otra máquina o de otra corrida), sin el driver de TeXpresso.
Remove-Item (Join-Path $carpeta '.lv\drivers\lv-texpresso.tex') -ErrorAction SilentlyContinue
# El candado de la copia es el del libro de origen: si la app de quien corre el smoke lo tiene abierto, la del smoke sería «segunda».
Remove-Item (Join-Path $carpeta '.lv\lock') -ErrorAction SilentlyContinue
$estado = Join-Path $carpeta '.lv\state.json'
if (Test-Path $estado) {
    $j = Get-Content $estado -Raw -Encoding UTF8 | ConvertFrom-Json
    $j.PSObject.Properties.Remove('texpresso')
    [IO.File]::WriteAllText($estado, ($j | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding $false))
}

$env:LV_OPEN = $carpeta
$env:LV_SMOKE_OUT = $salida
$env:LV_CONFIG_DIR = $config
Remove-Item Env:LV_ROOT -ErrorAction SilentlyContinue
Remove-Item Env:LV_SMOKE_PAUSA_MS -ErrorAction SilentlyContinue

function Correr([string]$archivo) {
    $env:LV_SMOKE = 'texpresso'
    if ($archivo) { $env:LV_SMOKE_FILE = $archivo } else { Remove-Item Env:LV_SMOKE_FILE -ErrorAction SilentlyContinue }
    if (Test-Path $salida) { Remove-Item $salida }
    $t = Get-Date
    $p = Start-Process -FilePath $exe -PassThru
    $limite = (Get-Date).AddMinutes($LimiteMin)
    while (-not $p.HasExited -and (Get-Date) -lt $limite) { Start-Sleep -Milliseconds 300 }
    if (-not $p.HasExited) {
        cmd /c "taskkill /PID $($p.Id) /T /F > NUL 2>&1"
        Write-Host "TOPE de $LimiteMin minutos: se cortó la app." -ForegroundColor Red
    }
    $seg = [int]((Get-Date) - $t).TotalSeconds
    if (-not (Test-Path $salida)) { Write-Host "La app no dejó resultado ($salida)." -ForegroundColor Red; return $null }
    $r = Get-Content $salida -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($s in $r.pasos) {
        $marca = if ($s.ok) { 'ok   ' } else { 'FALLA' }
        $color = if ($s.ok) { 'Green' } else { 'Red' }
        Write-Host ('{0} {1,-118} {2,6:N1} s' -f $marca, $s.nombre, ($s.ms / 1000)) -ForegroundColor $color
        if ($s.detalle) { Write-Host "      -> $($s.detalle)" -ForegroundColor $color }
    }
    Write-Host ("({0} s)" -f $seg)
    return $r
}

# Que al salir de la app no quede nada en la distro: el puente ve cerrado su stdin y mata el grupo de TeXpresso.
function Esperar-Sin-Procesos([string]$cuando) {
    $t = Get-Date
    $n = Contar-Procesos
    while ($n -gt 0 -and ((Get-Date) - $t).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 500; $n = Contar-Procesos }
    $ms = [int]((Get-Date) - $t).TotalMilliseconds
    if ($n -gt 0) { Write-Host "Después de $cuando quedaron $n procesos de TeXpresso en $Distro." -ForegroundColor Red; return $false }
    Write-Host "Después de ${cuando}: 0 procesos de TeXpresso en $Distro (a los $ms ms)." -ForegroundColor Green
    return $true
}

Write-Host "Smoke de TeXpresso sobre una copia del libro (distro $Distro)..."
$r1 = Correr ''
$ok1 = ($r1 -ne $null) -and $r1.ok
$limpio1 = Esperar-Sin-Procesos 'salir de la app'
$guardado = (Get-Content $estado -Raw -Encoding UTF8 | ConvertFrom-Json).texpresso -eq $true
if (-not $guardado) { Write-Host 'state.json no quedó con texpresso: true después de la primera corrida.' -ForegroundColor Red }

Write-Host ''
Write-Host 'Segunda corrida: el interruptor guardado prendido lanza TeXpresso al abrir...'
$r2 = Correr 'reabrir'
$ok2 = ($r2 -ne $null) -and $r2.ok
$limpio2 = Esperar-Sin-Procesos 'salir de la app (segunda corrida)'

Write-Host ''
if ($r1) { Write-Host ('medidas: ' + (($r1 | Select-Object prenderMs, arranqueMs, medianaMs, cambios, rafaga, detectarCuelgueMs, recuperarMs, buildMs, apagarMs | ConvertTo-Json -Compress -Depth 4))) }
if ($r2) { Write-Host ('reabrir: ' + (($r2 | Select-Object reabrirMs | ConvertTo-Json -Compress))) }
if ($ok1 -and $ok2 -and $limpio1 -and $limpio2 -and $guardado) { Write-Host 'SMOKE OK' -ForegroundColor Green; exit 0 }
$motivo = @($r1.fallo, $r2.fallo) | Where-Object { $_ }
Write-Host "SMOKE FALLÓ: $($motivo -join ' / ')" -ForegroundColor Red
exit 1
