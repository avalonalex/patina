#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>
int main(void){
  size_t sz = (size_t)16 << 30;
  for (int i=0;i<6;i++){
    void *p = mmap(NULL, sz, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0);
    printf("16GiB RW reservation %d at %p  (<2^35? %s)\n", i, p, ((uintptr_t)p < ((uintptr_t)1<<35)) ? "YES" : "no");
  }
  void *q = mmap(NULL, (size_t)2<<20, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0);
  printf("2MiB mapping at %p (<2^35? %s)\n", q, ((uintptr_t)q < ((uintptr_t)1<<35)) ? "YES":"no");
  return 0;
}
