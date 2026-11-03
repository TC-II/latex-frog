/* Costo de un fork() en Linux (WSL2) con un proceso del tamaño del XeTeX de TeXpresso: la línea de base.
 *
 * Reserva MB megas, los toca (residentes y sucios), y mide N veces: fork, el hijo ensucia DIRTY_KB y sale, el padre
 * espera. Reporta el fork solo (hasta que vuelve en el padre) y el ciclo completo con las fallas de copy-on-write.
 *
 * gcc -O2 -o fork_linux fork_linux.c && ./fork_linux 160 2048
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/wait.h>

static double now_ms(void)
{
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv)
{
  size_t mb = argc > 1 ? atoi(argv[1]) : 160;
  size_t dirty_kb = argc > 2 ? atoi(argv[2]) : 1024;
  size_t len = mb << 20;
  char *mem = malloc(len);
  memset(mem, 1, len);

  enum { N = 30 };
  double fork_ms[N], ciclo_ms[N];
  for (int i = 0; i < N; i++)
  {
    double t0 = now_ms();
    pid_t pid = fork();
    if (pid == 0)
    {
      for (size_t k = 0; k < dirty_kb * 1024; k += 4096)
        mem[(k * 7919) % len] ^= 1;
      _exit(0);
    }
    double t1 = now_ms();
    int st;
    waitpid(pid, &st, 0);
    double t2 = now_ms();
    fork_ms[i] = t1 - t0;
    ciclo_ms[i] = t2 - t0;
  }
  double a = 0, b = 0;
  for (int i = 0; i < N; i++) { a += fork_ms[i]; b += ciclo_ms[i]; }
  printf("Linux fork, %zu MB residentes, hijo ensucia %zu KB: fork %.3f ms, ciclo completo %.3f ms (promedio de %d)\n",
         mb, dirty_kb, a / N, b / N, N);
  return 0;
}
