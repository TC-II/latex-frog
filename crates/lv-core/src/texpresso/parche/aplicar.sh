#!/usr/bin/env bash
# Aplica a TeXpresso el parche de la vista «En vivo» y lo compila. Se corre ADENTRO de la distro de WSL:
#
#   wsl -d Ubuntu -e bash "/mnt/c/.../latex-frog/crates/lv-core/src/texpresso/parche/aplicar.sh" [carpeta-de-texpresso]
#
# La carpeta por omisión es ~/lv-prueba/texpresso (uno de los lugares donde el puente busca el binario). El parche es contra el
# commit e8df770 de https://github.com/let-def/texpresso; si el upstream se movió y no aplica, el script lo dice y no toca nada.
# Guarda el binario anterior como build/texpresso.sin-parche.
set -euo pipefail

aqui="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tp="${1:-$HOME/lv-prueba/texpresso}"
parche="$aqui/vista-en-vivo.patch"

[ -d "$tp/src/frontend" ] || { echo "No encuentro TeXpresso en $tp" >&2; exit 2; }
cd "$tp"

if grep -q 'live-page' src/frontend/editor.c; then
  echo "El parche ya está aplicado en $tp."
else
  git apply --check "$parche" || { echo "El parche no aplica sobre este TeXpresso (¿otro commit?)." >&2; exit 3; }
  git apply "$parche"
  echo "Parche aplicado."
fi

[ -f build/texpresso ] && [ ! -f build/texpresso.sin-parche ] && ! grep -q 'live-page' build/texpresso && cp build/texpresso build/texpresso.sin-parche
make -j"$(nproc)" >/dev/null
grep -q 'live-page' build/texpresso && echo "Listo: $tp/build/texpresso tiene la vista en vivo."
