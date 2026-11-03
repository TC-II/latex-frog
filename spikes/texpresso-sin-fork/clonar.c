/* Prototipo: el fork() "de verdad" de Windows, RtlCloneUserProcess (ntdll, no documentado).
 *
 * Clona el proceso con copy-on-write del espacio de direcciones, como fork(). Lo que se mide, con un proceso del tamaño
 * del XeTeX de TeXpresso (167 MB escribibles, residentes y sucios):
 *   1. cuánto tarda un clon (hasta que vuelve en el padre, y el ciclo completo: el hijo ensucia memoria, escribe en
 *      un pipe heredado y termina);
 *   2. qué puede hacer el hijo: malloc, WriteFile en un pipe heredado, abrir y leer un archivo;
 *   3. una cadena de 30 clones vivos a la vez (cada clon clona al siguiente y espera, como los snapshots de
 *      TeXpresso), y cuánto suma al commit del sistema: Windows no sobrecompromete memoria, así que cada clon carga
 *      sus 167 MB privados al commit aunque no los copie.
 *
 * Compilar (MinGW-w64):  gcc -O2 -Wno-format -o clonar.exe clonar.c
 */
#include <windows.h>
#include <psapi.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef LONG NTSTATUS;
#define STATUS_PROCESS_CLONED 0x00000129
#define RTL_CLONE_PROCESS_FLAGS_INHERIT_HANDLES 0x00000002

typedef struct
{
  ULONG Length;
  HANDLE ProcessHandle;
  HANDLE ThreadHandle;
  struct { HANDLE UniqueProcess, UniqueThread; } ClientId;
  BYTE ImageInformation[128]; /* SECTION_IMAGE_INFORMATION, con margen */
} RTL_USER_PROCESS_INFORMATION;

typedef NTSTATUS(NTAPI *RtlCloneUserProcess_t)(ULONG, PVOID, PVOID, HANDLE, RTL_USER_PROCESS_INFORMATION *);
typedef NTSTATUS(NTAPI *NtTerminateProcess_t)(HANDLE, NTSTATUS);

static RtlCloneUserProcess_t clone_fn;
static NtTerminateProcess_t term_fn;

#define STATE_MB 167
static char *mem;
static size_t mem_len = (size_t)STATE_MB << 20;
static HANDLE pipe_r, pipe_w;
static char self_path[MAX_PATH];

static double qpc_ms(void)
{
  static LARGE_INTEGER f;
  LARGE_INTEGER c;
  if (!f.QuadPart) QueryPerformanceFrequency(&f);
  QueryPerformanceCounter(&c);
  return (double)c.QuadPart * 1000.0 / (double)f.QuadPart;
}

static void child_exit(void)
{
  term_fn((HANDLE)-1, 0);
}

static void dirty(size_t kb)
{
  for (size_t k = 0; k < kb * 1024; k += 4096)
    mem[(k * 7919) % mem_len] ^= 1;
}

static NTSTATUS do_clone(RTL_USER_PROCESS_INFORMATION *info)
{
  memset(info, 0, sizeof(*info));
  info->Length = sizeof(*info);
  return clone_fn(RTL_CLONE_PROCESS_FLAGS_INHERIT_HANDLES, NULL, NULL, NULL, info);
}

static double commit_mb(void)
{
  PERFORMANCE_INFORMATION pi = {sizeof(pi)};
  GetPerformanceInfo(&pi, sizeof(pi));
  return (double)pi.CommitTotal * pi.PageSize / 1048576.0;
}

/* --- 1 y 2: clones sueltos --- */
static void single_clones(size_t dirty_kb)
{
  enum { N = 20 };
  double fork_sum = 0, cycle_sum = 0, fork_max = 0;
  int ok = 0;
  for (int i = 0; i < N; i++)
  {
    RTL_USER_PROCESS_INFORMATION info;
    double t0 = qpc_ms();
    NTSTATUS st = do_clone(&info);
    if (st == STATUS_PROCESS_CLONED)
    {
      /* hijo: nada de printf (la consola es del padre) */
      dirty(dirty_kb);
      char *p = malloc(1 << 20);
      memset(p, 7, 1 << 20);
      free(p);
      char msg[4] = {'o', 'k', 0, 0};
      HANDLE f = CreateFileA(self_path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
      if (f != INVALID_HANDLE_VALUE)
      {
        char mz[2];
        DWORD rd = 0;
        ReadFile(f, mz, 2, &rd, NULL);
        if (rd == 2 && mz[0] == 'M' && mz[1] == 'Z') msg[2] = 'f';
        CloseHandle(f);
      }
      DWORD wr;
      WriteFile(pipe_w, msg, 4, &wr, NULL);
      child_exit();
    }
    if (st < 0)
    {
      printf("RtlCloneUserProcess falló: 0x%08lx\n", (unsigned long)st);
      return;
    }
    double t1 = qpc_ms();
    char msg[4];
    DWORD rd = 0;
    ReadFile(pipe_r, msg, 4, &rd, NULL);
    WaitForSingleObject(info.ProcessHandle, INFINITE);
    double t2 = qpc_ms();
    CloseHandle(info.ProcessHandle);
    CloseHandle(info.ThreadHandle);
    if (rd == 4 && msg[0] == 'o' && msg[2] == 'f') ok++;
    fork_sum += t1 - t0;
    cycle_sum += t2 - t0;
    if (t1 - t0 > fork_max) fork_max = t1 - t0;
  }
  printf("clon suelto, %d MB residentes, el hijo ensucia %zu KB: clon %.2f ms (máx %.2f), ciclo completo %.2f ms; "
         "%d/%d hijos pudieron usar malloc, abrir un archivo y escribir en el pipe\n",
         STATE_MB, dirty_kb, fork_sum / N, fork_max, cycle_sum / N, ok, N);
}

/* --- 3: cadena de clones vivos --- */
static void chain(int depth)
{
  double c0 = commit_mb();
  double t0 = qpc_ms();
  for (int d = 0; d < depth; d++)
  {
    RTL_USER_PROCESS_INFORMATION info;
    NTSTATUS st = do_clone(&info);
    if (st == STATUS_PROCESS_CLONED)
    {
      dirty(200); /* lo que TeX ensucia entre dos snapshots (mediana medida: ~70 KB; acá algo más) */
      unsigned char b = (unsigned char)d;
      DWORD wr;
      WriteFile(pipe_w, &b, 1, &wr, NULL);
      if (d == depth - 1)
      {
        Sleep(3000); /* el último espera a que el de arriba mida el commit con todos vivos */
        child_exit();
      }
      continue; /* este clon clona al siguiente */
    }
    if (st < 0)
    {
      printf("cadena: el clon %d falló: 0x%08lx\n", d, (unsigned long)st);
      if (d > 0) child_exit();
      return;
    }
    if (d == 0)
    {
      /* el proceso original: lee los bytes de toda la cadena y mide */
      unsigned char b;
      DWORD rd;
      int got = 0;
      while (got < depth && ReadFile(pipe_r, &b, 1, &rd, NULL) && rd == 1) got++;
      double t1 = qpc_ms();
      double c1 = commit_mb();
      printf("cadena de %d clones vivos: %d respondieron, %.2f ms por clon; commit del sistema +%.0f MB (%.0f MB por clon)\n",
             depth, got, (t1 - t0) / depth, c1 - c0, (c1 - c0) / depth);
      WaitForSingleObject(info.ProcessHandle, INFINITE);
      return;
    }
    /* clon intermedio: espera a su hijo, como un snapshot de TeXpresso */
    WaitForSingleObject(info.ProcessHandle, INFINITE);
    child_exit();
  }
}

int main(void)
{
  setvbuf(stdout, NULL, _IONBF, 0);
  HMODULE nt = GetModuleHandleA("ntdll.dll");
  clone_fn = (RtlCloneUserProcess_t)GetProcAddress(nt, "RtlCloneUserProcess");
  term_fn = (NtTerminateProcess_t)GetProcAddress(nt, "NtTerminateProcess");
  if (!clone_fn || !term_fn) { printf("no está RtlCloneUserProcess\n"); return 1; }
  GetModuleFileNameA(NULL, self_path, MAX_PATH);

  SECURITY_ATTRIBUTES sa = {sizeof(sa), NULL, TRUE};
  CreatePipe(&pipe_r, &pipe_w, &sa, 0);

  mem = VirtualAlloc(NULL, mem_len, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
  memset(mem, 1, mem_len);

  single_clones(40);
  single_clones(2048);
  single_clones(6144);
  chain(30);
  return 0;
}
