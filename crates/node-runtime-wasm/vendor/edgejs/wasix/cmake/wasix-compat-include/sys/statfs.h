#ifndef WASIX_COMPAT_SYS_STATFS_H
#define WASIX_COMPAT_SYS_STATFS_H

#include <errno.h>

#ifdef __cplusplus
extern "C" {
#endif

struct statfs {
  unsigned long f_type;
  unsigned long f_bsize;
  unsigned long f_blocks;
  unsigned long f_bfree;
  unsigned long f_bavail;
  unsigned long f_files;
  unsigned long f_ffree;
};

static inline int statfs(const char* path, struct statfs* buf) {
  (void) path;
  if (buf != 0) {
    buf->f_type = 0;
    buf->f_bsize = 0;
    buf->f_blocks = 0;
    buf->f_bfree = 0;
    buf->f_bavail = 0;
    buf->f_files = 0;
    buf->f_ffree = 0;
  }
  errno = ENOSYS;
  return -1;
}

static inline int fstatfs(int fd, struct statfs* buf) {
  (void) fd;
  return statfs(0, buf);
}

#ifdef __cplusplus
}
#endif

#endif  // WASIX_COMPAT_SYS_STATFS_H
