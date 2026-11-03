#!/usr/bin/env python3
"""Mide, en WSL, cuántos procesos (snapshots) arma TeXpresso con el libro y cuánta memoria tiene cada uno.

Corre TeXpresso sobre ~/lv-prueba/libro (xdriver.tex, el parcial de Butterworth), espera a que el primer ciclo se
quede quieto, y lee /proc de cada texpresso-xetex vivo: RSS, Pss, Private_Dirty (lo que el proceso ya copió desde su
fork: es lo que costaría un snapshot incremental) y el tamaño del heap. Después hace un cambio en Butterworth y vuelve
a mirar. El stderr de TeXpresso (las líneas «[process] ...», «before process decimation», «rolling back») queda en
~/lv-prueba/tp-medir.log.

Uso (en WSL):  python3 medir_procesos.py
"""
import json, os, subprocess, threading, time

LIN = os.path.expanduser("~/lv-prueba/libro")
TP = os.path.expanduser("~/lv-prueba/texpresso/build/texpresso")
LOG = os.path.expanduser("~/lv-prueba/tp-medir.log")
QUIETO = 0.4

ultimo = [time.time()]


def leer(p):
    for _ in p.stdout:
        ultimo[0] = time.time()


def esperar_quieto(desde, limite=120):
    while time.time() - desde < limite:
        time.sleep(0.02)
        if time.time() - ultimo[0] >= QUIETO and ultimo[0] > desde:
            return ultimo[0] - desde
    return None


def campos(pid, archivo):
    out = {}
    try:
        with open(f"/proc/{pid}/{archivo}") as f:
            for linea in f:
                partes = linea.split()
                if len(partes) >= 2 and partes[0].endswith(":"):
                    try:
                        out[partes[0][:-1]] = int(partes[1])
                    except ValueError:
                        pass
    except OSError:
        pass
    return out


def heap_kb(pid):
    try:
        with open(f"/proc/{pid}/maps") as f:
            total = 0
            for linea in f:
                partes = linea.split()
                rango = partes[0].split("-")
                tam = (int(rango[1], 16) - int(rango[0], 16)) // 1024
                # heap y mapeos anónimos escribibles (malloc grande usa mmap)
                if len(partes) == 5 or partes[-1] == "[heap]":
                    if "w" in partes[1]:
                        total += tam
            return total
    except OSError:
        return 0


def procesos():
    res = []
    for d in os.listdir("/proc"):
        if not d.isdigit():
            continue
        try:
            with open(f"/proc/{d}/comm") as f:
                comm = f.read().strip()
        except OSError:
            continue
        if comm.startswith("texpresso-xet"):
            r = campos(d, "smaps_rollup")
            res.append((int(d), r.get("Rss", 0), r.get("Pss", 0), r.get("Private_Dirty", 0), heap_kb(d)))
    return sorted(res)


def informe(titulo):
    ps = procesos()
    print(f"== {titulo}: {len(ps)} procesos texpresso-xetex")
    print("   pid      RSS(KB)  Pss(KB)  PrivDirty(KB)  anon-rw(KB)")
    for pid, rss, pss, pd, hp in ps:
        print(f"   {pid:<8} {rss:>8} {pss:>8} {pd:>13} {hp:>12}")
    if ps:
        print(f"   suma Pss: {sum(p[2] for p in ps)} KB; suma PrivDirty: {sum(p[3] for p in ps)} KB")
    return ps


def main():
    env = dict(os.environ, PATH=os.path.dirname(TP) + ":/usr/bin:/bin")
    log = open(LOG, "w")
    p = subprocess.Popen([TP, "-texlive", "-json", "xdriver.tex"], cwd=LIN, env=env, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=log, text=True, encoding="utf-8")
    threading.Thread(target=leer, args=(p,), daemon=True).start()
    t0 = time.time()
    d = esperar_quieto(t0)
    print(f"primer ciclo: {d:.2f} s" if d else "primer ciclo: no se quedó quieto", flush=True)
    informe("después del primer ciclo")

    rel = "Secciones/Aproximación de Butterworth.tex"
    base = open(os.path.join(LIN, rel), encoding="utf-8").read()
    marca = "Esta es la función de aproximación de"
    for i in range(3):
        texto = base.replace(marca, f"Esta es (medida {i}) la función de aproximación de", 1)
        t = time.time()
        p.stdin.write(json.dumps(["open", os.path.join(LIN, rel), texto], ensure_ascii=False) + "\n")
        p.stdin.flush()
        d = esperar_quieto(t, 30)
        print(f"cambio {i}: {d*1000:.0f} ms" if d else f"cambio {i}: sin respuesta", flush=True)
        informe(f"después del cambio {i}")
    p.terminate()
    log.close()
    with open(LOG) as f:
        texto = f.read()
    print("forks (CHLD) vistos en el log:", texto.count("[process] launched"), "lanzados;",
          texto.count("rolling back"), "rollbacks;", texto.count("before process decimation"), "decimaciones")


if __name__ == "__main__":
    main()
