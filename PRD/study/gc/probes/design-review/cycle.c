#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
static double now(void){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec*1e3+t.tv_nsec/1e6; }
int main(int argc,char**argv){
  int mode=atoi(argv[1]); size_t cyc=(size_t)8<<20, blk=32<<10, keep=256<<10; int rounds=200;
  char*p=mmap(0,(size_t)16<<30,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_NORESERVE,-1,0);
  double t0=now(), tdec=0;
  for(int r=0;r<rounds;r++){
    /* "allocation": write every word of the 8 MiB cycle (constructors write every word) */
    for(size_t off=0;off<cyc;off+=8) *(volatile long*)(p+off)=off;
    double a=now();
    if(mode==1) for(size_t off=keep;off<cyc;off+=blk) mmap(p+off,blk,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0);
    if(mode==2) mmap(p+keep,cyc-keep,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0);
    tdec+=now()-a;
  }
  double t1=now();
  printf("mode %d: %d cycles of 8 MiB: total %.1f ms (%.3f ms/cycle), of which decommit calls %.3f ms/cycle\n",mode,rounds,t1-t0,(t1-t0)/rounds,tdec/rounds);
  return 0;}
