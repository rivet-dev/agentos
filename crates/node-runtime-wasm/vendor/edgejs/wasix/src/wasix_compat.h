#ifndef WASIX_COMPAT_H
#define WASIX_COMPAT_H

#include <errno.h>
#include <grp.h>
#include <net/if.h>
#include <netdb.h>
#include <pthread.h>
#include <sched.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#if defined(__wasi__)
int getgroups(int size, gid_t list[]);

static inline pid_t wasix_fork(void) {
  errno = ENOSYS;
  return -1;
}
#define fork wasix_fork

static inline unsigned int wasix_if_nametoindex(const char* ifname) {
  (void)ifname;
  return 0;
}
#define if_nametoindex wasix_if_nametoindex

static inline char* wasix_if_indextoname(unsigned int ifindex, char* ifname) {
  (void)ifindex;
  if (ifname != NULL) ifname[0] = '\0';
  errno = ENXIO;
  return NULL;
}
#define if_indextoname wasix_if_indextoname

static inline int wasix_getservbyport_r(
    int port,
    const char* proto,
    struct servent* se,
    char* buf,
    size_t buflen,
    struct servent** result) {
  (void)port;
  (void)proto;
  (void)se;
  (void)buf;
  (void)buflen;
  if (result != NULL) *result = NULL;
  errno = ENOSYS;
  return ENOSYS;
}
#define getservbyport_r wasix_getservbyport_r

static char* wasix_tzname[2] = {(char*)"UTC", (char*)"UTC"};
static long wasix_timezone = 0;
static int wasix_daylight = 0;
#define tzname wasix_tzname
#define timezone wasix_timezone
#define daylight wasix_daylight
#endif

#ifndef SCM_RIGHTS
#define SCM_RIGHTS 0x01
#endif

#ifndef CMSG_FIRSTHDR
struct cmsghdr {
  socklen_t cmsg_len;
  int cmsg_level;
  int cmsg_type;
};

#define WASIX_CMSG_ALIGN(len) (((len) + sizeof(size_t) - 1) & ~(sizeof(size_t) - 1))
#define CMSG_DATA(cmsg) ((unsigned char*)((struct cmsghdr*)(cmsg) + 1))
#define CMSG_SPACE(len) (WASIX_CMSG_ALIGN(sizeof(struct cmsghdr)) + WASIX_CMSG_ALIGN(len))
#define CMSG_LEN(len) (WASIX_CMSG_ALIGN(sizeof(struct cmsghdr)) + (len))
#define CMSG_FIRSTHDR(mhdr)                                                     \
  ((size_t)(mhdr)->msg_controllen >= sizeof(struct cmsghdr)                     \
       ? (struct cmsghdr*)(mhdr)->msg_control                                   \
       : (struct cmsghdr*)0)
#define CMSG_NXTHDR(mhdr, cmsg) ((struct cmsghdr*)0)
#endif

#ifndef PRIO_PROCESS
#define PRIO_PROCESS 0
#endif

static inline int getpriority(int which, int who) {
  (void)which;
  (void)who;
  return 0;
}

static inline int setpriority(int which, int who, int prio) {
  (void)which;
  (void)who;
  (void)prio;
  return -1;
}

#ifndef SCHED_OTHER
#define SCHED_OTHER 0
#endif

static inline int wasix_pthread_getschedparam(pthread_t t, int* policy, struct sched_param* sp) {
  (void)t;
  if (policy != NULL) *policy = SCHED_OTHER;
  if (sp != NULL) sp->sched_priority = 0;
  return 0;
}

static inline int wasix_pthread_setschedparam(pthread_t t, int policy, const struct sched_param* sp) {
  (void)t;
  (void)policy;
  (void)sp;
  return 0;
}

static inline int wasix_sched_get_priority_min(int policy) {
  (void)policy;
  return 0;
}

static inline int wasix_sched_get_priority_max(int policy) {
  (void)policy;
  return 0;
}

#define pthread_getschedparam wasix_pthread_getschedparam
#define pthread_setschedparam wasix_pthread_setschedparam
#define sched_get_priority_min wasix_sched_get_priority_min
#define sched_get_priority_max wasix_sched_get_priority_max

static inline int wasix_pthread_getname_np(pthread_t t, char* name, size_t len) {
  (void)t;
  if (name != NULL && len > 0) name[0] = '\0';
  return 0;
}
#define pthread_getname_np wasix_pthread_getname_np

static inline char* wasix_ptsname(int fd) {
  (void)fd;
  errno = ENOSYS;
  return (char*)0;
}
#define ptsname wasix_ptsname

// Exported guest allocator used by the WASIX N-API bridge.
#ifdef __cplusplus
extern "C" {
#endif
uint32_t unofficial_napi_guest_malloc(uint32_t size);
#ifdef __cplusplus
}
#endif

#endif  // WASIX_COMPAT_H
