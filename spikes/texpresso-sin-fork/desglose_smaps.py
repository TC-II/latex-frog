#!/usr/bin/env python3
"""Desglosa la memoria de un proceso texpresso-xetex (en WSL) por tipo de mapeo, desde /proc/<pid>/smaps.

Separa lo que un snapshot explícito tendría que copiar (memoria anónima escribible y .data/.bss del ejecutable) de lo
que no (código, bibliotecas de sólo lectura, archivos mapeados como el .fmt o fuentes).

Uso (en WSL):  python3 desglose_smaps.py <pid>
"""
import sys
from collections import defaultdict

pid = sys.argv[1]
grupos = defaultdict(lambda: [0, 0, 0])  # Rss, Private_Dirty, Size (KB)
actual = None
with open(f"/proc/{pid}/smaps") as f:
    for linea in f:
        partes = linea.split()
        if "-" in partes[0] and len(partes[0]) > 8 and ":" not in partes[0]:
            perms = partes[1]
            nombre = partes[5] if len(partes) > 5 else "[anon]"
            rango = partes[0].split("-")
            tam = (int(rango[1], 16) - int(rango[0], 16)) // 1024
            if nombre.startswith("/"):
                base = nombre.rsplit("/", 1)[-1]
                tipo = ("escribible " if "w" in perms else "sólo lectura ") + base
                if "w" not in perms:
                    tipo = "archivos/código sólo lectura"
            else:
                tipo = nombre + (" rw" if "w" in perms else " ro")
            actual = tipo
            grupos[actual][2] += tam
        elif partes[0] == "Rss:":
            grupos[actual][0] += int(partes[1])
        elif partes[0] == "Private_Dirty:":
            grupos[actual][1] += int(partes[1])

print(f"{'mapeo':<50} {'Size KB':>10} {'Rss KB':>10} {'PrivDirty':>10}")
for k, (rss, pd, size) in sorted(grupos.items(), key=lambda kv: -kv[1][0]):
    if rss > 0:
        print(f"{k:<50} {size:>10} {rss:>10} {pd:>10}")
tot = sum(v[0] for v in grupos.values())
esc = sum(v[0] for k, v in grupos.items() if " rw" in k or k.startswith("escribible"))
print(f"total Rss {tot} KB; escribible residente (lo que un snapshot completo copia) {esc} KB")
