/*
 * decommit.c: what giving back a large free region costs in system calls,
 * by the size of the pieces it is given back in (PRD/GC_PRD.md §7; review
 * finding PERF-4; #650). 464 MiB is the empty capacity a library load
 * leaves (#647's table), and decommit runs in the pause unless deferred.
 *
 * 464 MiB is touched in a 16 GiB MAP_NORESERVE mapping and given back in
 * pieces of BLOCK KiB by:
 *   0  mmap(MAP_FIXED) of fresh anonymous memory over each piece
 *   1  madvise(MADV_FREE_REUSABLE) on macOS, madvise(MADV_DONTNEED) on Linux
 *   2  madvise(MADV_FREE), then mprotect(PROT_NONE)
 * Reports the time per call and the resident size after, then the time to
 * touch the 464 MiB again.
 *
 * Build and run, macOS or Linux:
 *   cc -O2 -o /tmp/decommit scripts/gc_probes/decommit.c
 *   for b in 32 4096; do for m in 0 1 2; do /tmp/decommit $m $b; done; done
 *
 * Result, Apple M4 Pro, macOS 27.2, 2026-10-07, three runs (ms of calls
 * for the 464 MiB):
 *                         32 KiB pieces   4 MiB pieces
 *   0 mmap(MAP_FIXED)        19-31           3.8-5.1
 *   1 MADV_FREE_REUSABLE     11-13           2.8-3.3     resident unchanged
 *   2 MADV_FREE + PROT_NONE  22-27           4.5
 * Touching it all again: 19-32 ms. Coalescing into 4 MiB runs cuts the
 * calls' time 4-6x. The study's run measured 14.6 ms against 1.8 ms (8x).
 *
 * Result, Linux 6.8 arm64 in a colima VM on the same machine (4 KiB pages,
 * overcommit 0), 2026-10-07, three runs, where mode 1 is
 * MADV_DONTNEED:
 *                         32 KiB pieces   4 MiB pieces
 *   0 mmap(MAP_FIXED)        32-38           8.6-10.5
 *   1 MADV_DONTNEED          18-21           8.4-8.7
 *   2 MADV_FREE + PROT_NONE  20-25           6.1-6.2     resident unchanged
 * Touching it all again: 57-108 ms. Coalescing cuts MADV_DONTNEED's calls
 * about 2.3x here.
 */
#if defined(__linux__)
#define _GNU_SOURCE
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#if defined(__APPLE__)
#include <mach/mach.h>
static double rss_mb(void) {
  struct mach_task_basic_info info;
  mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
  task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&info, &count);
  return info.resident_size / 1048576.0;
}
#define GIVE_BACK MADV_FREE_REUSABLE
#else
static double rss_mb(void) {
  long size = 0, resident = 0;
  FILE *f = fopen("/proc/self/statm", "r");
  if (f) {
    if (fscanf(f, "%ld %ld", &size, &resident) != 2) resident = 0;
    fclose(f);
  }
  return resident * (double)sysconf(_SC_PAGESIZE) / 1048576.0;
}
#define GIVE_BACK MADV_DONTNEED
#endif

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: %s MODE (0-2) BLOCK_KIB\n", argv[0]);
    return 2;
  }
  int mode = atoi(argv[1]);
  size_t total = (size_t)464 << 20, block = (size_t)atoi(argv[2]) << 10;
  if (mode < 0 || mode > 2 || block == 0 || total % block) {
    fprintf(stderr, "MODE is 0-2 and BLOCK_KIB divides 464 MiB\n");
    return 2;
  }
  char *p = mmap(0, (size_t)16 << 30, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
  if (p == MAP_FAILED) {
    perror("mmap");
    return 1;
  }
  memset(p, 1, total);
  double t0 = now_ms();
  int bad = 0;
  for (size_t off = 0; off < total; off += block) {
    if (mode == 0)
      bad |= mmap(p + off, block, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) !=
             p + off;
    if (mode == 1) bad |= madvise(p + off, block, GIVE_BACK);
    if (mode == 2) {
      bad |= madvise(p + off, block, MADV_FREE);
      bad |= mprotect(p + off, block, PROT_NONE);
    }
  }
  double t1 = now_ms();
  printf("mode %d, %zu KiB pieces (%zu calls): %.1f ms, %.2f us per call; resident after %.0f MB%s\n", mode,
         block >> 10, total / block, t1 - t0, (t1 - t0) * 1e3 / (total / block), rss_mb(),
         bad ? " (a call failed)" : "");
  if (mode == 2) mprotect(p, total, PROT_READ | PROT_WRITE);
  double t2 = now_ms();
  memset(p, 2, total);
  printf("   touching the 464 MiB again: %.1f ms\n", now_ms() - t2);
  return 0;
}
