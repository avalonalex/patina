#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <mach/mach.h>
static double rss_mb(void){ struct mach_task_basic_info i; mach_msg_type_number_t c=MACH_TASK_BASIC_INFO_COUNT; task_info(mach_task_self(),MACH_TASK_BASIC_INFO,(task_info_t)&i,&c); return i.resident_size/1048576.0; }
static double foot_mb(void){ task_vm_info_data_t v; mach_msg_type_number_t c=TASK_VM_INFO_COUNT; task_info(mach_task_self(),TASK_VM_INFO,(task_info_t)&v,&c); return v.phys_footprint/1048576.0; }
int main(int argc,char**argv){
  size_t n=(size_t)256<<20; int mode=atoi(argv[1]);
  char*p=mmap(0,(size_t)16<<30,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_NORESERVE,-1,0);
  memset(p,1,n);
  printf("mode %d touched: rss=%.0f footprint=%.0f\n",mode,rss_mb(),foot_mb());
  int r=0;
  if(mode==0) r=madvise(p,n,MADV_FREE);
  if(mode==1) r=madvise(p,n,MADV_DONTNEED);
  if(mode==2) r=madvise(p,n,MADV_FREE_REUSABLE);
  if(mode==3){ r=madvise(p,n,MADV_FREE); r|=mprotect(p,n,PROT_NONE); }
  if(mode==4){ void*q=mmap(p,n,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0); r=(q!=p); }
  printf("mode %d after (r=%d): rss=%.0f footprint=%.0f\n",mode,r,rss_mb(),foot_mb());
  return 0;}
