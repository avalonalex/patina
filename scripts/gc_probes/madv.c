/*
 * madv.c: how each way of handing memory back to the OS shows in the
 * resident size and the physical footprint (PRD/GC_PRD.md §7, decommit;
 * #650). The collector's decommit (stage 5e) rests on this result.
 *
 * 256 MiB is touched in a 16 GiB PROT_READ|PROT_WRITE, MAP_NORESERVE
 * mapping, then given back by one of:
 *   0  madvise(MADV_FREE)
 *   1  madvise(MADV_DONTNEED)
 *   2  madvise(MADV_FREE_REUSABLE)                   (macOS only)
 *   3  madvise(MADV_FREE), then mprotect(PROT_NONE)
 *   4  mmap(MAP_FIXED) of fresh anonymous memory over the range
 * Resident size (and on macOS the physical footprint) is read before and
 * after.
 *
 * Build and run, macOS or Linux:
 *   cc -O2 -o /tmp/madv scripts/gc_probes/madv.c
 *   for m in 0 1 2 3 4; do /tmp/madv $m; done
 *
 * Result, Apple M4 Pro, macOS 27.2, 2026-10-07 (MB, resident / footprint):
 *   0 MADV_FREE                258/257 -> 258/257
 *   1 MADV_DONTNEED            258/257 -> 258/257
 *   2 MADV_FREE_REUSABLE       258/257 -> 258/1
 *   3 MADV_FREE + PROT_NONE    258/257 -> 2/1
 *   4 mmap(MAP_FIXED)          258/257 -> 2/1
 * The same as the study's run of 2026-10-01: on macOS only modes 3 and 4
 * return resident memory at once, and mode 2 returns only the footprint.
 *
 * Result, Linux 6.8 arm64 in a colima VM on the same machine (4 KiB pages,
 * overcommit 0), 2026-10-07 (MB resident):
 *   0 MADV_FREE                257 -> 257
 *   1 MADV_DONTNEED            256 -> 1
 *   3 MADV_FREE + PROT_NONE    257 -> 257
 *   4 mmap(MAP_FIXED)          257 -> 1
 * On Linux MADV_DONTNEED returns memory at once and PROT_NONE does not
 * help: PRD/GC_PRD.md §7 decommits with MADV_DONTNEED on Linux and
 * mmap(MAP_FIXED) on macOS.
 */
#if defined(__linux__)
#define _GNU_SOURCE
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#if defined(__APPLE__)
#include <mach/mach.h>
static double rss_mb(void) {
  struct mach_task_basic_info info;
  mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
  task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&info, &count);
  return info.resident_size / 1048576.0;
}
static double footprint_mb(void) {
  task_vm_info_data_t info;
  mach_msg_type_number_t count = TASK_VM_INFO_COUNT;
  task_info(mach_task_self(), TASK_VM_INFO, (task_info_t)&info, &count);
  return info.phys_footprint / 1048576.0;
}
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
static double footprint_mb(void) { return -1; }
#endif

static void report(const char *when, int mode) {
  double footprint = footprint_mb();
  if (footprint >= 0)
    printf("mode %d %s: resident %.0f MB, footprint %.0f MB\n", mode, when, rss_mb(), footprint);
  else
    printf("mode %d %s: resident %.0f MB\n", mode, when, rss_mb());
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: %s MODE (0-4)\n", argv[0]);
    return 2;
  }
  int mode = atoi(argv[1]);
  size_t n = (size_t)256 << 20;
  char *p = mmap(0, (size_t)16 << 30, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
  if (p == MAP_FAILED) {
    perror("mmap");
    return 1;
  }
  memset(p, 1, n);
  report("touched", mode);
  int r = 0;
  switch (mode) {
  case 0: r = madvise(p, n, MADV_FREE); break;
  case 1: r = madvise(p, n, MADV_DONTNEED); break;
  case 2:
#if defined(MADV_FREE_REUSABLE)
    r = madvise(p, n, MADV_FREE_REUSABLE);
#else
    printf("mode 2: MADV_FREE_REUSABLE is macOS only\n");
    return 0;
#endif
    break;
  case 3: r = madvise(p, n, MADV_FREE); r |= mprotect(p, n, PROT_NONE); break;
  case 4: r = mmap(p, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) != p; break;
  default: fprintf(stderr, "mode must be 0-4\n"); return 2;
  }
  if (r) printf("mode %d: the call failed\n", mode);
  report("after", mode);
  return 0;
}
