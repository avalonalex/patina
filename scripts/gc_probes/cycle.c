/*
 * cycle.c: what decommitting a cycle's memory costs the next cycle
 * (PRD/GC_PRD.md §7, decommit; review finding PERF-4; #650). If the
 * collector gave back the allocation budget after each collection, every
 * cycle would fault it in again.
 *
 * In a 16 GiB MAP_NORESERVE mapping, 200 cycles each write every word of
 * 8 MiB (as constructors write every word they allocate), then give back
 * all but the first 256 KiB by:
 *   0  nothing
 *   1  mmap(MAP_FIXED) over each 32 KiB block in turn
 *   2  one mmap(MAP_FIXED) over the whole range
 * and report the time per cycle and the time in the decommit calls.
 *
 * Build and run, macOS or Linux:
 *   cc -O2 -o /tmp/cycle scripts/gc_probes/cycle.c
 *   for m in 0 1 2; do /tmp/cycle $m; done
 *
 * Result, Apple M4 Pro, macOS 27.2, 2026-10-07, two runs: 0.31 ms per
 * cycle without decommit; 0.90 ms decommitting each 32 KiB block (0.27 ms of
 * it in the calls); 0.63-0.68 ms in one coalesced call (0.04-0.05 ms in
 * it). So a re-committed MiB costs about 40-75 us. The study's run measured
 * 0.23-0.32, 0.71 and 0.51 ms: the same order, about 1.3x slower here.
 *
 * Result, Linux 6.8 arm64 in a colima VM on the same machine (4 KiB pages,
 * overcommit 0), 2026-10-07, two runs: 0.30 ms without
 * decommit; 1.61-1.67 ms per block (0.50-0.52 ms in the calls); 1.15-1.25
 * ms coalesced (0.12-0.13 ms): about 110-175 us per re-committed MiB, a
 * VM's page faults included.
 */
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <time.h>

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: %s MODE (0-2)\n", argv[0]);
    return 2;
  }
  int mode = atoi(argv[1]);
  size_t cycle = (size_t)8 << 20, block = 32 << 10, keep = 256 << 10;
  int rounds = 200;
  char *p = mmap(0, (size_t)16 << 30, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
  if (p == MAP_FAILED) {
    perror("mmap");
    return 1;
  }
  double t0 = now_ms(), decommit = 0;
  for (int r = 0; r < rounds; r++) {
    for (size_t off = 0; off < cycle; off += 8) *(volatile long *)(p + off) = (long)off;
    double a = now_ms();
    if (mode == 1)
      for (size_t off = keep; off < cycle; off += block)
        mmap(p + off, block, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (mode == 2)
      mmap(p + keep, cycle - keep, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    decommit += now_ms() - a;
  }
  double t1 = now_ms();
  printf("mode %d: %d cycles of 8 MiB: %.3f ms per cycle, %.3f ms of it in decommit calls\n", mode, rounds,
         (t1 - t0) / rounds, decommit / rounds);
  return 0;
}
