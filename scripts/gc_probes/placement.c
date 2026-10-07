/*
 * placement.c: where anonymous mappings land, whether a hint far above
 * them is honoured, and how many large reservations a process can hold
 * (PRD/GC_PRD.md §5, the placement rule S5 that tells an arena index from a
 * block address, and K13, many heaps' reservations; #650). It extends the
 * study's `mm.c`, which checked the first two.
 *
 * Maps, without a hint, 2 MiB and 64 MiB regions and six 16 GiB
 * MAP_NORESERVE reservations, and reports whether each lies below 2^35
 * (where arena references live today); then asks for 64 MiB at a hint of
 * 2^40 without MAP_FIXED and reports whether it was placed there exactly;
 * then holds as many 16 GiB MAP_NORESERVE reservations as it can, up to
 * RESERVATIONS (default 4,096).
 *
 * Build and run, macOS or Linux:
 *   cc -O2 -o /tmp/placement scripts/gc_probes/placement.c
 *   /tmp/placement            # or /tmp/placement 4096
 *
 * Result, Apple M4 Pro, macOS 27.2, 2026-10-07: 2 MiB at 0x1054f8000 and
 * 64 MiB at 0x1056f8000, both below 2^35; the 16 GiB reservations from
 * 0x7000000000 up, above it; the hint at 2^40 honoured exactly; 4,096 more
 * 16 GiB reservations held at once. The study's run found the same.
 *
 * Result, Linux 6.8 arm64 in a colima VM on the same machine (4 KiB pages,
 * overcommit 0), 2026-10-07: every unhinted map lands at the
 * top of the address space (2 MiB at 0xe24c3de00000, the 16 GiB
 * reservations below it), above 2^35; the hint at 2^40 honoured exactly;
 * 4,096 more 16 GiB reservations held at once under heuristic overcommit.
 * So the hint, not the default placement, is what puts blocks where the
 * placement rule expects them on both systems.
 */
#if defined(__linux__)
#define _GNU_SOURCE
#endif
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>

static const uintptr_t below = (uintptr_t)1 << 35;

static void *anon(void *hint, size_t size, int flags) {
  return mmap(hint, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | flags, -1, 0);
}

static void place(const char *what, void *p) {
  if (p == MAP_FAILED)
    printf("%-30s failed\n", what);
  else
    printf("%-30s at %p (%s 2^35)\n", what, p, (uintptr_t)p < below ? "below" : "above");
}

int main(int argc, char **argv) {
  int reservations = argc > 1 ? atoi(argv[1]) : 4096;
  size_t huge = (size_t)16 << 30;
  place("2 MiB", anon(0, (size_t)2 << 20, 0));
  place("64 MiB", anon(0, (size_t)64 << 20, 0));
  for (int i = 0; i < 6; i++) {
    char what[40];
    snprintf(what, sizeof what, "16 GiB reservation %d", i);
    place(what, anon(0, huge, MAP_NORESERVE));
  }
  void *hint = (void *)((uintptr_t)1 << 40);
  void *q = anon(hint, (size_t)64 << 20, 0);
  printf("%-30s at %p: %s\n", "64 MiB at a hint of 2^40", q,
         q == hint ? "the hint was honoured" : "the hint was not honoured");
  int held = 0;
  while (held < reservations && anon(0, huge, MAP_NORESERVE) != MAP_FAILED) held++;
  printf("%d of %d further 16 GiB reservations held at once\n", held, reservations);
  return 0;
}
