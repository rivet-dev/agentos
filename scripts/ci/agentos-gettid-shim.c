#define _GNU_SOURCE
#include <sys/syscall.h>
#include <sys/types.h>
#include <unistd.h>

#ifndef SYS_gettid
#  if defined(__x86_64__)
#    define SYS_gettid 186
#  elif defined(__aarch64__)
#    define SYS_gettid 178
#  elif defined(__arm__)
#    define SYS_gettid 224
#  elif defined(__i386__)
#    define SYS_gettid 224
#  elif defined(__powerpc64__)
#    define SYS_gettid 207
#  else
#    error "gettid syscall number unknown for this architecture"
#  endif
#endif

pid_t gettid(void) {
  return (pid_t)syscall(SYS_gettid);
}
