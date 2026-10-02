#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <mach/mach.h>
static double rss_mb(void){ struct mach_task_basic_info i; mach_msg_type_number_t c=MACH_TASK_BASIC_INFO_COUNT; task_info(mach_task_self(),MACH_TASK_BASIC_INFO,(task_info_t)&i,&c); return i.resident_size/1048576.0; }
static double now(void){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec*1e3+t.tv_nsec/1e6; }
int main(int argc,char**argv){
  size_t total=(size_t)464<<20, blk=(size_t)atoi(argv[2])<<10; int mode=atoi(argv[1]);
  char*p=mmap(0,(size_t)16<<30,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_NORESERVE,-1,0);
  memset(p,1,total);
  double t0=now(); int bad=0;
  for(size_t off=0; off<total; off+=blk){
    if(mode==0){ if(mmap(p+off,blk,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0)!=p+off) bad++; }
    if(mode==1){ bad|=madvise(p+off,blk,MADV_FREE_REUSABLE); }
    if(mode==2){ bad|=madvise(p+off,blk,MADV_FREE); bad|=mprotect(p+off,blk,PROT_NONE); }
  }
  double t1=now();
  printf("mode %d blk=%zuKiB n=%zu: %.1f ms (%.2f us/call) rss_after=%.0f MB bad=%d\n",mode,blk>>10,total/blk,t1-t0,(t1-t0)*1e3/(total/blk),rss_mb(),bad);
  /* reuse cost: touch again */
  if(mode==2) mprotect(p,total,PROT_READ|PROT_WRITE);
  double t2=now(); memset(p,2,total); double t3=now();
  printf("   re-touch 464 MiB: %.1f ms\n",t3-t2);
  return 0;}
