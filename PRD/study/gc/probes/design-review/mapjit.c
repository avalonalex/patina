#include <sys/mman.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <pthread.h>
#include <libkern/OSCacheControl.h>
int main(int argc, char**argv){
  size_t sz = (size_t)strtoull(argv[1],0,0);
  int n = atoi(argv[2]);
  int ok=0;
  void *first=0;
  for(int i=0;i<n;i++){
    void*p=mmap(0,sz,PROT_READ|PROT_WRITE|PROT_EXEC,MAP_PRIVATE|MAP_ANON|MAP_JIT,-1,0);
    if(p==MAP_FAILED){printf("fail at %d errno=%d %s\n",i,errno,strerror(errno));break;}
    if(!first) first=p;
    ok++;
    if(i<3 || i==n-1){
      // write a ret and execute it
      pthread_jit_write_protect_np(0);
      unsigned int *c=(unsigned int*)p; c[0]=0xd2800540; /* mov x0,#42 */ c[1]=0xd65f03c0; /* ret */
      pthread_jit_write_protect_np(1);
      sys_icache_invalidate(p,8);
      long (*f)(void)=(long(*)(void))p;
      printf("region %d at %p returns %ld\n",i,p,f());
    }
  }
  printf("ok=%d size=%zu\n",ok,sz);
  return 0;
}
