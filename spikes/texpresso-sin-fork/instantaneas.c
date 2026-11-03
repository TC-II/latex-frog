/* Prototipo: snapshots de un "motor" dentro del mismo proceso, en Windows, sin fork().
 *
 * Es lo que haría falta para reemplazar los fork() de TeXpresso: el motor (acá uno sintético, calibrado con lo que se
 * midió del XeTeX de TeXpresso sobre el libro: ~167 MB escribibles residentes, entre 40 KB y 6 MB ensuciados entre un
 * snapshot y el siguiente) vive entero en una región reservada con MEM_WRITE_WATCH. Windows lleva la cuenta de qué
 * páginas se escribieron (GetWriteWatch), así que un snapshot copia sólo las páginas sucias desde el anterior (un
 * delta hacia adelante) más la pila del motor, y guarda los registros con __builtin_setjmp. Volver al snapshot k es
 * reponer las páginas que cambiaron después de k (las de los deltas posteriores y las sucias de ahora), reponer la
 * pila y saltar con __builtin_longjmp.
 *
 * El motor lee el documento línea por línea (como TeX pide bytes con txp_read); cada lectura es un punto donde el
 * controlador puede tomar un snapshot. Las líneas que abren y cierran grupos hacen recursión, con variables locales que
 * influyen en el resultado: si la pila no se repusiera bien, el hash final cambiaría.
 *
 * Escenario: corre la variante 0 del documento, después edita (variantes 1 a 5) y para cada edición vuelve al último
 * snapshot anterior a la primera línea distinta y sigue desde ahí. Imprime el hash de cada variante, que tiene que
 * coincidir con el de una corrida limpia (`instantaneas.exe --solo v`, sin snapshots).
 *
 * Compilar (MinGW-w64, el gcc de Strawberry Perl):  gcc -O2 -Wno-format -o instantaneas.exe instantaneas.c
 * Correr:  instantaneas.exe [--pesimista]   y para verificar:  instantaneas.exe [--pesimista] --solo 0..5
 */
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <malloc.h>

#define PAGE 4096
#define REGION_MB 320
#define STATE_MB 167
#define REGION_BYTES ((size_t)REGION_MB << 20)
#define NPAGES (REGION_BYTES / PAGE)
#define STATE_PAGES (((size_t)STATE_MB << 20) / PAGE)
#define NLINES 4000
#define SNAP_EVERY 100   /* líneas entre snapshots: ~40 snapshots en el documento, como los ~30 de TeXpresso */
#define MAX_SNAPS 256
#define NVARIANTS 6

/* ---------- reloj ---------- */
static double qpc_ms(void)
{
  static LARGE_INTEGER f;
  LARGE_INTEGER c;
  if (!f.QuadPart) QueryPerformanceFrequency(&f);
  QueryPerformanceCounter(&c);
  return (double)c.QuadPart * 1000.0 / (double)f.QuadPart;
}

/* ---------- región del motor ---------- */
static char *region;
typedef struct { uint64_t rng, acc; int line; } globals_t;
#define G ((globals_t *)region)
#define HEAP (region + PAGE)

/* ---------- documento (lo tiene el controlador, como el VFS de TeXpresso) ---------- */
static char doc[NVARIANTS][NLINES][64];
static int cur_variant;
static const int edit_line[NVARIANTS] = {0, 3900, 3900, 2000, 150, 3901};

static uint64_t fnv(const char *s)
{
  uint64_t h = 1469598103934665603ULL;
  for (; *s; s++) h = (h ^ (unsigned char)*s) * 1099511628211ULL;
  return h;
}

static void make_docs(void)
{
  uint64_t r = 12345;
  int depth = 0;
  for (int i = 0; i < NLINES; i++)
  {
    r ^= r << 13; r ^= r >> 7; r ^= r << 17;
    char c = 'a';
    if (depth < 40 && r % 7 == 0) { c = '{'; depth++; }
    else if (depth > 0 && r % 7 == 1) { c = '}'; depth--; }
    snprintf(doc[0][i], 64, "%c linea %d palabra %llu", c, i, (unsigned long long)(r % 100000));
  }
  for (int v = 1; v < NVARIANTS; v++)
  {
    memcpy(doc[v], doc[v - 1], sizeof(doc[0]));
    int l = edit_line[v];
    doc[v][l][1] = 0;
    char tmp[64];
    snprintf(tmp, 64, "%c editada en la variante %d", doc[v - 1][l][0], v);
    memcpy(doc[v][l], tmp, 64);
  }
}

/* ---------- controlador: snapshots ---------- */
typedef struct
{
  int line;              /* línea que el motor iba a leer cuando se tomó */
  intptr_t jb[5];        /* __builtin_setjmp */
  char *stack_low;       /* pila guardada: [stack_low, engine_top) */
  size_t stack_len;
  char *stack_copy;
  uint32_t npages;       /* delta: páginas sucias desde el snapshot anterior, en orden */
  uint32_t *pages;
  char *data;
  uint8_t *bitmap;       /* NPAGES bits */
  size_t out_len;
} snap_t;

static snap_t snaps[MAX_SNAPS];
static int nsnaps;
static char *engine_top;
static int solo_mode;

/* el snapshot 0 es una imagen de todas las páginas tocadas hasta ese momento */
static int32_t *base_slot;  /* página -> índice en base_data, o -1 (página nunca tocada: ceros) */
static char *base_data;
static size_t base_pages;

static uint64_t out_log[100000];
static size_t out_len;

static void **ww_addrs;

/* estadísticas */
static double snap_ms_sum, snap_ms_max, base_ms;
static size_t snap_kb_sum, snap_kb_max, snap_count;
static double full_copy_ms;

static ULONG_PTR get_dirty(int reset)
{
  ULONG_PTR count = NPAGES;
  ULONG gran;
  if (GetWriteWatch(reset ? WRITE_WATCH_FLAG_RESET : 0, region, REGION_BYTES, ww_addrs, &count, &gran) != 0)
  {
    fprintf(stderr, "GetWriteWatch falló\n");
    exit(1);
  }
  return count;
}

/* Se probó devolver la dirección de una local y gcc -O2 la cambia por NULL (-Wreturn-local-addr): se lee rsp. */
static inline __attribute__((always_inline)) char *approx_sp(void)
{
  char *sp;
  __asm__ volatile("mov %%rsp, %0" : "=r"(sp));
  return sp;
}

__attribute__((noinline)) static void save_snapshot_body(snap_t *s)
{
  double t0 = qpc_ms();
  s->stack_low = (char *)(((uintptr_t)approx_sp() - 256) & ~(uintptr_t)15);
  s->stack_len = engine_top - s->stack_low;
  s->stack_copy = malloc(s->stack_len);
  memcpy(s->stack_copy, s->stack_low, s->stack_len);
  s->out_len = out_len;

  ULONG_PTR n = get_dirty(1);
  if (nsnaps == 0)
  {
    base_slot = malloc(NPAGES * sizeof(int32_t));
    for (size_t p = 0; p < NPAGES; p++) base_slot[p] = -1;
    base_data = malloc((size_t)n * PAGE);
    for (ULONG_PTR i = 0; i < n; i++)
    {
      size_t p = ((char *)ww_addrs[i] - region) / PAGE;
      base_slot[p] = (int32_t)i;
      memcpy(base_data + i * PAGE, region + p * PAGE, PAGE);
    }
    base_pages = n;
    s->npages = 0;
    s->bitmap = calloc(NPAGES / 8, 1);
    base_ms = qpc_ms() - t0;
    return;
  }
  s->npages = (uint32_t)n;
  s->pages = malloc(n * sizeof(uint32_t));
  s->data = malloc((size_t)n * PAGE);
  s->bitmap = calloc(NPAGES / 8, 1);
  for (ULONG_PTR i = 0; i < n; i++)
  {
    uint32_t p = (uint32_t)(((char *)ww_addrs[i] - region) / PAGE);
    s->pages[i] = p;
    s->bitmap[p >> 3] |= 1 << (p & 7);
    memcpy(s->data + i * PAGE, region + (size_t)p * PAGE, PAGE);
  }
  double dt = qpc_ms() - t0;
  size_t kb = (size_t)n * PAGE / 1024 + s->stack_len / 1024;
  snap_ms_sum += dt; if (dt > snap_ms_max) snap_ms_max = dt;
  snap_kb_sum += kb; if (kb > snap_kb_max) snap_kb_max = kb;
  snap_count++;
}

__attribute__((noinline)) static void take_snapshot(void)
{
  snap_t *s = &snaps[nsnaps];
  s->line = G->line;
  if (__builtin_setjmp(s->jb))
    return; /* volvimos acá desde restore(): el motor sigue leyendo el documento nuevo */
  save_snapshot_body(s);
  nsnaps++;
}

static void free_snap(snap_t *s)
{
  free(s->stack_copy); free(s->pages); free(s->data); free(s->bitmap);
  memset(s, 0, sizeof(*s));
}

static const char *find_page(int k, uint32_t p)
{
  for (int j = k; j >= 1; j--)
  {
    snap_t *s = &snaps[j];
    if (s->bitmap[p >> 3] & (1 << (p & 7)))
    {
      uint32_t lo = 0, hi = s->npages;
      while (lo < hi) { uint32_t m = (lo + hi) / 2; if (s->pages[m] < p) lo = m + 1; else hi = m; }
      return s->data + (size_t)lo * PAGE;
    }
  }
  return base_slot[p] >= 0 ? base_data + (size_t)base_slot[p] * PAGE : NULL;
}

static double restore_ms;
static size_t restore_pages;
static uint8_t *restore_mark;

__attribute__((noinline, noreturn)) static void restore_inner(int k)
{
  double t0 = qpc_ms();
  if (!restore_mark) restore_mark = malloc(NPAGES / 8);
  memset(restore_mark, 0, NPAGES / 8);
  size_t count = 0;
  /* páginas sucias desde el último snapshot */
  ULONG_PTR n = get_dirty(0);
  for (ULONG_PTR i = 0; i < n; i++)
  {
    uint32_t p = (uint32_t)(((char *)ww_addrs[i] - region) / PAGE);
    restore_mark[p >> 3] |= 1 << (p & 7);
  }
  /* y las de los deltas posteriores a k */
  for (int j = k + 1; j < nsnaps; j++)
    for (uint32_t i = 0; i < snaps[j].npages; i++)
    {
      uint32_t p = snaps[j].pages[i];
      restore_mark[p >> 3] |= 1 << (p & 7);
    }
  for (uint32_t p = 0; p < NPAGES; p++)
    if (restore_mark[p >> 3] & (1 << (p & 7)))
    {
      const char *src = find_page(k, p);
      if (src) memcpy(region + (size_t)p * PAGE, src, PAGE);
      else memset(region + (size_t)p * PAGE, 0, PAGE);
      count++;
    }
  get_dirty(1); /* lo que acabamos de escribir no cuenta como sucio */
  for (int j = k + 1; j < nsnaps; j++) free_snap(&snaps[j]);
  nsnaps = k + 1;
  out_len = snaps[k].out_len;
  memcpy(snaps[k].stack_low, snaps[k].stack_copy, snaps[k].stack_len);
  restore_ms = qpc_ms() - t0;
  restore_pages = count;
  __builtin_longjmp(snaps[k].jb, 1);
}

__attribute__((noinline, noreturn)) static void restore(int k)
{
  /* Bajar la pila por debajo de la zona que se va a reponer antes de pisarla. */
  char *sp = approx_sp();
  char *target = snaps[k].stack_low - 16384;
  if (sp > target)
  {
    volatile char *pad = alloca(sp - target);
    pad[0] = 0;
  }
  restore_inner(k);
}

/* ---------- motor ---------- */
static uint64_t next_rand(void)
{
  G->rng ^= G->rng << 13; G->rng ^= G->rng >> 7; G->rng ^= G->rng << 17;
  return G->rng;
}

static void ctl_on_eof(void);

static int read_line(char *buf)
{
  if (!solo_mode && (nsnaps == 0 || G->line - snaps[nsnaps - 1].line >= SNAP_EVERY))
    take_snapshot();
  if (G->line >= NLINES)
    return -1;
  memcpy(buf, doc[cur_variant][G->line], 64);
  return 1;
}

static void touch_line(const char *s)
{
  uint64_t h = fnv(s);
  int pages = 1 + (int)(h % 3);
  size_t hot = ((size_t)G->line * 37) % (STATE_PAGES - 64);
  for (int i = 0; i < pages; i++)
  {
    size_t p = hot + next_rand() % 64;
    uint64_t *w = (uint64_t *)(HEAP + p * PAGE + (next_rand() % 512) * 8);
    *w = *w * 31 + h;
    G->acc ^= *w;
  }
}

/* Perfil de escritura. "real" se calibró con el XeTeX de TeXpresso sobre el libro (Private_Dirty de cada snapshot:
 * mediana ~70 KB, algunos de 1 a 6 MB, ~37 MB sumando ~30 snapshots): los shipouts tocan una zona de 40 MB (la parte
 * dinámica de mem). "pesimista" ensucia ~5 veces más y en los 167 MB enteros. */
static int pessimistic;

static void shipout(uint64_t local)
{
  /* una página de salida: TeX libera y reusa mucha memoria de golpe */
  int pages = pessimistic ? 64 + (int)(next_rand() % 1400) : 16 + (int)(next_rand() % 300);
  size_t zone = pessimistic ? STATE_PAGES - 1 : (40u << 20) / PAGE;
  for (int i = 0; i < pages; i++)
  {
    size_t p = next_rand() % zone;
    uint64_t *w = (uint64_t *)(HEAP + p * PAGE + (next_rand() % 512) * 8);
    *w ^= local + i;
  }
  out_log[out_len++] = G->acc ^ local;
}

static int run(int depth)
{
  uint64_t group_sum = (uint64_t)depth * 1000003ULL;
  char buf[64];
  for (;;)
  {
    if (read_line(buf) < 0)
      return 0;
    G->line++;
    group_sum = group_sum * 1099511628211ULL ^ fnv(buf);
    touch_line(buf);
    if (G->line % 60 == 0) shipout(group_sum);
    if (buf[0] == '{') { if (!run(depth + 1)) return 0; }
    else if (buf[0] == '}' && depth > 0) { G->acc += group_sum; return 1; }
  }
}

static uint64_t result_hash(void)
{
  uint64_t h = G->acc;
  for (size_t i = 0; i < out_len; i++) h = h * 1099511628211ULL ^ out_log[i];
  return h;
}

static double run_t0;

static void ctl_on_eof(void)
{
  double t = qpc_ms() - run_t0;
  if (solo_mode)
  {
    printf("variante %d (corrida limpia): hash %016llx\n", cur_variant, (unsigned long long)result_hash());
    exit(0);
  }
  printf("variante %d: hash %016llx, motor %.1f ms", cur_variant, (unsigned long long)result_hash(), t);
  if (cur_variant > 0)
    printf(" (restauración: %zu páginas = %.1f MB repuestas en %.3f ms)", restore_pages,
           restore_pages * PAGE / 1048576.0, restore_ms);
  printf("\n");
  if (cur_variant + 1 == NVARIANTS)
  {
    size_t mem = base_pages * PAGE;
    for (int j = 1; j < nsnaps; j++) mem += (size_t)snaps[j].npages * PAGE + snaps[j].stack_len;
    printf("\nsnapshot base (todo lo tocado al empezar): %zu MB en %.1f ms\n", base_pages * PAGE >> 20, base_ms);
    printf("copia completa de %d MB, para comparar: %.1f ms\n", STATE_MB, full_copy_ms);
    printf("snapshots incrementales: %zu, promedio %.3f ms / %zu KB, máximo %.3f ms / %zu KB\n", snap_count,
           snap_ms_sum / snap_count, snap_kb_sum / snap_count, snap_ms_max, snap_kb_max);
    printf("memoria de los snapshots vivos al final (%d): %.1f MB\n", nsnaps, mem / 1048576.0);
    exit(0);
  }
  /* Edición: la primera línea distinta entre la variante actual y la próxima es la "valla" de TeXpresso. */
  int next = cur_variant + 1;
  int fence = 0;
  while (fence < NLINES && memcmp(doc[cur_variant][fence], doc[next][fence], 64) == 0) fence++;
  int k = nsnaps - 1;
  while (k > 0 && snaps[k].line > fence) k--;
  cur_variant = next;
  int line_k = snaps[k].line;
  run_t0 = qpc_ms();
  printf("  edición en la línea %d -> snapshot %d (línea %d)\n", fence, k, line_k);
  restore(k);
}

__attribute__((noinline)) static void engine_start(void)
{
  run(0);
  ctl_on_eof();
}

__attribute__((noinline)) static void enter_engine(void)
{
  volatile char top[64];
  top[0] = 0;
  engine_top = (char *)top;  /* la pila del motor es lo que está por debajo de acá */
  engine_start();
}

int main(int argc, char **argv)
{
  for (int i = 1; i < argc; i++)
  {
    if (strcmp(argv[i], "--solo") == 0 && i + 1 < argc)
    {
      solo_mode = 1;
      cur_variant = atoi(argv[++i]);
    }
    else if (strcmp(argv[i], "--pesimista") == 0)
      pessimistic = 1;
  }
  setvbuf(stdout, NULL, _IONBF, 0);
  make_docs();
  region = VirtualAlloc(NULL, REGION_BYTES, MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH, PAGE_READWRITE);
  if (!region) { fprintf(stderr, "VirtualAlloc falló\n"); return 1; }
  ww_addrs = malloc(NPAGES * sizeof(void *));

  /* "cargar el formato": dejar residentes y sucios los ~167 MB, como el XeTeX de TeXpresso */
  G->rng = 88172645463325252ULL;
  for (size_t p = 0; p < STATE_PAGES; p++)
    memset(HEAP + p * PAGE, (int)(p & 0xff), PAGE);

  if (!solo_mode)
  {
    char *tmp = malloc((size_t)STATE_MB << 20);
    memset(tmp, 0, (size_t)STATE_MB << 20);
    double t0 = qpc_ms();
    memcpy(tmp, HEAP, (size_t)STATE_MB << 20);
    full_copy_ms = qpc_ms() - t0;
    free(tmp);
  }
  run_t0 = qpc_ms();
  enter_engine();
  return 0;
}
