#!/usr/bin/env python3
"""Puente entre Latex Frog (Windows) y TeXpresso (WSL). Lo escribe y lo lanza `lv-core` (texpresso.rs) con
`wsl.exe -d <distro> --cd <carpeta del puente> -e python3 -u puente.py <orden> ...`; no se corre a mano.

Órdenes:
  sondear [--texpresso RUTA] [--raiz RUTA_WINDOWS]
      Imprime una línea JSON con lo que hay en la distro: python, TeXpresso (y su motor), TeX Live (kpsewhich y xelatex),
      la pantalla de WSLg y, con --raiz, si la carpeta del proyecto se ve desde Linux. No lanza nada.
  servir --raiz RUTA_WINDOWS --driver REL [--texpresso RUTA]
      Lanza `texpresso -texlive -json` sobre el proyecto y hace de intermediario:
      - stdin (de la app): una orden JSON por línea. `open`, `close` y `synctex-forward` llevan rutas relativas a la raíz del
        proyecto y el puente las vuelve absolutas dentro de la granja. `lv-ping-N` pasa tal cual: TeXpresso no lo conoce y lo
        dice por stderr (`unknown verb: lv-ping-N`), que es la prueba de que su bucle principal sigue leyendo. `lv-pausar` (sólo
        para el smoke) le manda SIGSTOP a TeXpresso para simular un cuelgue.
      - stdout (a la app): un JSON por línea. Los mensajes de TeXpresso que sirven (`append`/`truncate` del buffer `out`,
        `flush`, `synctex`, `reset-sync` y, con el parche «En vivo», `live-page`: la página como PNG en base64) y los del puente: `lv-hola` (arrancó), `lv-pong` (respondió un ping), `lv-error` (un
        `[fatal]` de TeXpresso) y `lv-salio` (terminó, con las últimas líneas de su stderr).
      - stdin cerrado (la app apagó el interruptor, cerró el proyecto o murió): mata el grupo de procesos de TeXpresso y sale.

Por qué una «granja» de enlaces y no el proyecto directo: TeXpresso resuelve todo relativo a la carpeta del documento y
rechaza un `open` de un archivo que no esté debajo de ella («file has a different root»), así que el driver no puede vivir en
`.lv/drivers/`. Escribirlo en la raíz del proyecto ensuciaría la carpeta del usuario. La granja es una carpeta en el disco de
Linux (`~/.cache/latex-frog/<hash>/`) con un enlace a cada entrada de la raíz del proyecto (por `/mnt/c`) y una copia del driver, que
la app escribe en `.lv/drivers/lv-texpresso.tex`. Medido con el libro: el primer ciclo tarda lo mismo que sobre una copia en
Linux (1,2 a 1,5 s); lo lento de `/mnt/c` sólo se nota la primera vez que se lee cada archivo.
"""
import hashlib, json, os, re, shutil, signal, subprocess, sys, threading, time

DRIVER = "lv-texpresso.tex"
CANDIDATOS = ["~/.local/bin/texpresso", "~/texpresso/build/texpresso", "~/lv-prueba/texpresso/build/texpresso",
              "/opt/texpresso/build/texpresso", "/usr/local/bin/texpresso"]
PING = re.compile(r"unknown verb: lv-ping-(\d+)")

_salida = threading.Lock()


def emitir(m):
    linea = json.dumps(m, ensure_ascii=False)
    with _salida:
        sys.stdout.write(linea + "\n")
        sys.stdout.flush()


def arg(nombre, defecto=None):
    if nombre in sys.argv:
        i = sys.argv.index(nombre)
        if i + 1 < len(sys.argv):
            return sys.argv[i + 1]
    return defecto


def buscar_texpresso(pedido):
    if pedido:
        p = os.path.expanduser(pedido)
        return p if os.path.isfile(p) and os.access(p, os.X_OK) else None
    w = shutil.which("texpresso")
    if w:
        return w
    for c in CANDIDATOS:
        p = os.path.expanduser(c)
        if os.path.isfile(p) and os.access(p, os.X_OK):
            return p
    return None


def ruta_linux(win):
    """La ruta de Windows vista desde Linux (`wslpath -u`); None si no se puede traducir o no existe."""
    try:
        r = subprocess.run(["wslpath", "-u", win], capture_output=True, text=True, timeout=10)
    except Exception:
        return None
    p = r.stdout.strip()
    return p if r.returncode == 0 and p and os.path.isdir(p) else None


def path_de(tp):
    # Sin la parte de Windows que WSL agrega al PATH: kpsewhich no la necesita y cada búsqueda por /mnt/c es lenta.
    return os.path.dirname(tp) + ":/usr/local/bin:/usr/bin:/bin"


def tiene_parche_vivo(tp):
    """El TeXpresso es el que lleva el parche de la vista «En vivo» (`texpresso/parche/`): su binario conoce el mensaje `live-page`."""
    if not tp:
        return False
    try:
        with open(tp, "rb") as f:
            return b"live-page" in f.read()
    except OSError:
        return False


def sondear():
    tp = buscar_texpresso(arg("--texpresso"))
    env_path = path_de(tp) if tp else "/usr/local/bin:/usr/bin:/bin"
    which = lambda n: shutil.which(n, path=env_path)
    info = {
        "python": sys.version.split()[0],
        "texpresso": tp,
        "motor": bool(tp and os.access(os.path.join(os.path.dirname(tp), "texpresso-xetex"), os.X_OK)),
        "kpsewhich": which("kpsewhich"),
        "xelatex": which("xelatex"),
        "pantalla": bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")),
        "vivo": tiene_parche_vivo(tp),
        "distro": None,
    }
    try:
        with open("/etc/os-release", encoding="utf-8") as f:
            for l in f:
                if l.startswith("PRETTY_NAME="):
                    info["distro"] = l.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    raiz = arg("--raiz")
    if raiz is not None:
        info["raiz"] = ruta_linux(raiz)
    emitir(info)


def armar_granja(raiz_linux, driver_rel):
    h = hashlib.md5(raiz_linux.encode("utf-8")).hexdigest()[:12]
    granja = os.path.join(os.path.expanduser("~/.cache/latex-frog"), h)
    os.makedirs(granja, exist_ok=True)
    # Sólo se borran enlaces: si alguien dejó un archivo de verdad acá, se respeta.
    for e in os.scandir(granja):
        if e.is_symlink():
            os.unlink(e.path)
    for e in os.scandir(raiz_linux):
        if e.name == DRIVER:
            continue
        try:
            os.symlink(e.path, os.path.join(granja, e.name))
        except FileExistsError:
            pass
    # El driver va copiado y no enlazado: TeXpresso resuelve el enlace del documento y tomaría `.lv/drivers` como su carpeta (se
    # probó: «document path: .../.lv/drivers» y todo el resto «not found»). Los cambios de perfil le llegan igual, como un `open`.
    destino = os.path.join(granja, DRIVER)
    if os.path.lexists(destino):
        os.unlink(destino)
    shutil.copyfile(os.path.join(raiz_linux, driver_rel), destino)
    return granja


def matar_anterior(granja):
    """Un TeXpresso que quedó de una corrida anterior sobre el mismo proyecto (la app murió sin cerrar stdin)."""
    try:
        with open(os.path.join(granja, ".pid")) as f:
            pgid = int(f.read().strip())
        with open(f"/proc/{pgid}/cmdline", "rb") as f:
            if b"texpresso" not in f.read():
                return
        os.killpg(pgid, signal.SIGTERM)
        time.sleep(0.3)
        os.killpg(pgid, signal.SIGKILL)
    except (OSError, ValueError):
        pass


def servir():
    raiz_win = arg("--raiz")
    driver_rel = arg("--driver", ".lv/drivers/" + DRIVER)
    tp = buscar_texpresso(arg("--texpresso"))
    if not tp:
        emitir(["lv-error", "no se encontró TeXpresso en la distro"])
        emitir(["lv-salio", 127, []])
        return
    raiz = ruta_linux(raiz_win) if raiz_win else None
    if not raiz:
        emitir(["lv-error", f"la carpeta del proyecto no se ve desde Linux: {raiz_win}"])
        emitir(["lv-salio", 2, []])
        return
    granja = armar_granja(raiz, driver_rel)
    matar_anterior(granja)
    env = dict(os.environ, PATH=path_de(tp))
    p = subprocess.Popen([tp, "-texlive", "-json", DRIVER], cwd=granja, env=env, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    with open(os.path.join(granja, ".pid"), "w") as f:
        f.write(str(p.pid))

    terminando = threading.Event()

    def matar():
        terminando.set()
        try:
            os.killpg(p.pid, signal.SIGTERM)
        except OSError:
            return
        for _ in range(20):
            if p.poll() is not None:
                break
            time.sleep(0.05)
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except OSError:
            pass

    for s in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(s, lambda *_: (matar(), os._exit(0)))

    def absoluta(rel):
        return rel if rel.startswith("/") else os.path.join(granja, rel)

    def relativa(ruta):
        pre = granja.rstrip("/") + "/"
        return ruta[len(pre):] if ruta.startswith(pre) else ruta

    def entrada():
        for linea in sys.stdin.buffer:
            try:
                m = json.loads(linea.decode("utf-8"))
            except (ValueError, UnicodeDecodeError):
                continue
            if not isinstance(m, list) or not m:
                continue
            anotar("in", json.dumps(m[:2], ensure_ascii=False))
            if m[0] == "lv-pausar":
                try:
                    os.killpg(p.pid, signal.SIGSTOP)
                except OSError:
                    pass
                continue
            if m[0] in ("open", "close", "synctex-forward") and len(m) > 1 and isinstance(m[1], str):
                m[1] = absoluta(m[1])
            try:
                p.stdin.write((json.dumps(m, ensure_ascii=False) + "\n").encode("utf-8"))
                p.stdin.flush()
            except OSError:
                break
        # stdin cerrado: la app ya no quiere a TeXpresso.
        matar()
        os._exit(0)

    def salida():
        for linea in p.stdout:
            anotar("out", linea.decode("utf-8", "replace").rstrip())
            try:
                m = json.loads(linea.decode("utf-8", "replace"))
            except ValueError:
                continue
            if not isinstance(m, list) or not m:
                continue
            v = m[0]
            if v in ("append", "truncate") and len(m) > 1 and m[1] != "out":
                continue  # el buffer `log` repite al `out` con más ruido
            if v in ("append", "truncate", "flush", "reset-sync", "live-page"):
                emitir(m)  # `live-page`: la imagen de la página (parche «En vivo» de TeXpresso)
            elif v == "synctex" and len(m) >= 3:
                emitir(["synctex", relativa(m[1])] + m[2:])

    ultimas = []
    # `--traza` (la app lo pasa con LV_TEXPRESSO_TRAZA=1): todo lo que entra y sale, para diagnosticar. Queda en la granja.
    traza = open(os.path.join(granja, "traza.log"), "a", encoding="utf-8") if "--traza" in sys.argv else None
    _traza = threading.Lock()

    def anotar(que, t):
        if traza:
            with _traza:
                traza.write(f"{time.time():.3f} {que} {t[:400]}\n")
                traza.flush()

    def errores():
        for linea in p.stderr:
            t = linea.decode("utf-8", "replace").rstrip()
            anotar("err", t)
            g = PING.search(t)
            if g:
                emitir(["lv-pong", int(g.group(1))])
                continue
            if t.startswith("stdin:"):
                continue  # TeXpresso repite cada orden que lee: con un buffer entero no sirve
            ultimas.append(t[:300])
            del ultimas[:-30]
            if t.startswith("[fatal]"):
                emitir(["lv-error", t])

    for f in (entrada, salida, errores):
        threading.Thread(target=f, daemon=True).start()
    emitir(["lv-hola", {"pid": p.pid, "granja": granja, "texpresso": tp}])
    codigo = p.wait()
    time.sleep(0.1)
    if not terminando.is_set():
        emitir(["lv-salio", codigo, ultimas[-12:]])
    sys.exit(0)


if __name__ == "__main__":
    orden = sys.argv[1] if len(sys.argv) > 1 else ""
    if orden == "sondear":
        sondear()
    elif orden == "servir":
        servir()
    else:
        sys.stderr.write(__doc__)
        sys.exit(2)
