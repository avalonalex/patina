/*
 * mapjit.c: how many MAP_JIT regions a process can map, and whether code
 * written under per-thread write protection runs from each (PRD/GC_PRD.md
 * §14 and stage 6's code-memory interface; review finding JIT-10; #650).
 * macOS arm64 only: MAP_JIT and pthread_jit_write_protect_np are Apple's.
 *
 * Maps COUNT regions of SIZE bytes with MAP_JIT, and in the first three and
 * the last writes `mov x0, #42; ret` with writes enabled for this thread,
 * re-protects, flushes the instruction cache and calls it.
 *
 * Build and run (macOS arm64; no entitlement is needed outside the
 * hardened runtime):
 *   cc -O2 -o /tmp/mapjit scripts/gc_probes/mapjit.c
 *   /tmp/mapjit 0x10000000 4096    # 4,096 regions of 256 MiB
 *   /tmp/mapjit 0x40000000 400     # 400 regions of 1 GiB
 *
 * Result, Apple M4 Pro, macOS 27.2, 2026-10-07: 4,096 regions of 256 MiB
 * and 400 of 1 GiB mapped, and each region called returned 42, as in the
 * study's run.
 */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#if defined(__APPLE__) && defined(__aarch64__)
#include <libkern/OSCacheControl.h>
#include <pthread.h>

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: %s SIZE COUNT\n", argv[0]);
    return 2;
  }
  size_t size = (size_t)strtoull(argv[1], 0, 0);
  int count = atoi(argv[2]), mapped = 0, ran = 0;
  for (int i = 0; i < count; i++) {
    void *p = mmap(0, size, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS | MAP_JIT, -1, 0);
    if (p == MAP_FAILED) {
      printf("region %d failed: %s\n", i, strerror(errno));
      break;
    }
    mapped++;
    if (i < 3 || i == count - 1) {
      pthread_jit_write_protect_np(0);
      unsigned int *code = p;
      code[0] = 0xd2800540; /* mov x0, #42 */
      code[1] = 0xd65f03c0; /* ret */
      pthread_jit_write_protect_np(1);
      sys_icache_invalidate(p, 8);
      long (*f)(void) = (long (*)(void))p;
      long answer = f();
      ran += answer == 42;
      printf("region %d at %p returns %ld\n", i, p, answer);
    }
  }
  printf("%d regions of %zu bytes mapped; %d of the ones called returned 42\n", mapped, size, ran);
  return 0;
}
#else
int main(void) {
  printf("mapjit: MAP_JIT and per-thread write protection are macOS arm64 only\n");
  return 0;
}
#endif
