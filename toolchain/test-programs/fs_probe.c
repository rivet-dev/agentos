/* fs_probe.c - deterministic native-vs-VM filesystem parity probe.
 *
 * argv[1] is a scratch directory. Run this once as a native Linux binary and
 * once inside the VM, then diff stdout. The output intentionally avoids file
 * descriptor numbers, absolute paths, uid/gid, and locale-dependent strings.
 */
#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

static const char *scratch_dir;
static char path_buffers[6][1024];
static unsigned path_index;
static unsigned failures;

static const char *path_for(const char *name) {
  char *buffer = path_buffers[path_index++ % 6];
  snprintf(buffer, 1024, "%s/%s", scratch_dir, name);
  return buffer;
}

static void cleanup(void) {
  unlink(path_for("mode.txt"));
  unlink(path_for("append.txt"));
  unlink(path_for("excl.txt"));
  unlink(path_for("rw.txt"));
  unlink(path_for("rename_src.txt"));
  unlink(path_for("rename_dst.txt"));
  unlink(path_for("link.txt"));
  unlink(path_for("trunc.txt"));
  unlink(path_for("large.bin"));
  rmdir(path_for("dir"));
}

static void report(const char *label, int ok) {
  printf("%-32s %s\n", label, ok ? "ok" : "FAIL");
  if (!ok) {
    failures++;
  }
}

static void expect_ok(const char *label, long ret) {
  int err = errno;
  int ok = ret >= 0;
  printf("%-32s %s errno=%d\n", label, ok ? "ok" : "FAIL", ok ? 0 : err);
  if (!ok) {
    failures++;
  }
}

static const char *expected_errno_name(int err) {
  if (err == EACCES) {
    return "EACCES";
  }
  if (err == EEXIST) {
    return "EEXIST";
  }
  return NULL;
}

static void expect_errno(const char *label, long ret, int expected_errno) {
  int err = errno;
  int ok = ret < 0 && err == expected_errno;
  const char *name = ret < 0 ? expected_errno_name(err) : NULL;
  if (name) {
    printf("%-32s %s errno=%s\n", label, ok ? "ok" : "FAIL", name);
  } else {
    printf("%-32s %s errno=%d\n", label, ok ? "ok" : "FAIL", ret < 0 ? err : 0);
  }
  if (!ok) {
    failures++;
  }
}

static int write_all(int fd, const void *data, size_t len) {
  const char *cursor = (const char *)data;
  while (len > 0) {
    ssize_t written = write(fd, cursor, len);
    if (written < 0) {
      return -1;
    }
    if (written == 0) {
      errno = EIO;
      return -1;
    }
    cursor += written;
    len -= (size_t)written;
  }
  return 0;
}

static int read_exact_file(const char *path, char *buffer, size_t len) {
  int fd = open(path, O_RDONLY);
  if (fd < 0) {
    return -1;
  }
  size_t offset = 0;
  while (offset < len) {
    ssize_t n = read(fd, buffer + offset, len - offset);
    if (n < 0) {
      int err = errno;
      close(fd);
      errno = err;
      return -1;
    }
    if (n == 0) {
      close(fd);
      errno = EIO;
      return -1;
    }
    offset += (size_t)n;
  }
  close(fd);
  return 0;
}

static void write_text_file(const char *path, const char *text, mode_t mode) {
  int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, mode);
  if (fd < 0) {
    return;
  }
  (void)write_all(fd, text, strlen(text));
  close(fd);
}

static void mode_line(const char *label, const char *path, mode_t expected_mode,
                      off_t expected_size, int expected_type) {
  struct stat st;
  errno = 0;
  if (stat(path, &st) != 0) {
    printf("%-32s FAIL errno=%d\n", label, errno);
    failures++;
    return;
  }
  int ok = ((st.st_mode & 07777) == expected_mode) &&
           (st.st_size == expected_size) &&
           ((st.st_mode & S_IFMT) == expected_type);
  printf("%-32s %s mode=%04o size=%lld type=%o\n", label,
         ok ? "ok" : "FAIL", (unsigned)(st.st_mode & 07777),
         (long long)st.st_size, (unsigned)(st.st_mode & S_IFMT));
  if (!ok) {
    failures++;
  }
}

static void fmode_line(const char *label, int fd, mode_t expected_mode,
                       off_t expected_size, int expected_type) {
  struct stat st;
  errno = 0;
  if (fstat(fd, &st) != 0) {
    printf("%-32s FAIL errno=%d\n", label, errno);
    failures++;
    return;
  }
  int ok = ((st.st_mode & 07777) == expected_mode) &&
           (st.st_size == expected_size) &&
           ((st.st_mode & S_IFMT) == expected_type);
  printf("%-32s %s mode=%04o size=%lld type=%o\n", label,
         ok ? "ok" : "FAIL", (unsigned)(st.st_mode & 07777),
         (long long)st.st_size, (unsigned)(st.st_mode & S_IFMT));
  if (!ok) {
    failures++;
  }
}

static void stat_and_access_cases(void) {
  const char *mode_path = path_for("mode.txt");
  int fd;

  errno = 0;
  fd = open(mode_path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  expect_ok("open_create_0644", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("fchmod_initial_0644", fchmod(fd, 0644));
    errno = 0;
    expect_ok("write_initial", write_all(fd, "hello", 5));
    fmode_line("fstat_initial_mode", fd, 0644, 5, S_IFREG);
    close(fd);
  }
  mode_line("stat_initial_mode", mode_path, 0644, 5, S_IFREG);

  errno = 0;
  expect_ok("access_F_OK", access(mode_path, F_OK));
  errno = 0;
  expect_ok("access_R_OK", access(mode_path, R_OK));
  errno = 0;
  expect_ok("access_W_OK", access(mode_path, W_OK));
  errno = 0;
  expect_errno("access_X_OK_no_exec", access(mode_path, X_OK), EACCES);

  errno = 0;
  expect_ok("chmod_0600", chmod(mode_path, 0600));
  mode_line("stat_after_chmod_0600", mode_path, 0600, 5, S_IFREG);

  errno = 0;
  fd = open(mode_path, O_RDWR);
  expect_ok("open_existing_RDWR", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("fchmod_0640", fchmod(fd, 0640));
    fmode_line("fstat_after_fchmod_0640", fd, 0640, 5, S_IFREG);
    close(fd);
  }
  mode_line("stat_after_fchmod_0640", mode_path, 0640, 5, S_IFREG);

  errno = 0;
  expect_ok("chmod_0755", chmod(mode_path, 0755));
  errno = 0;
  expect_ok("access_X_OK_exec", access(mode_path, X_OK));
}

static void create_write_cases(void) {
  const char *append_path = path_for("append.txt");
  const char *excl_path = path_for("excl.txt");
  const char *rw_path = path_for("rw.txt");
  char buffer[16] = {0};
  int fd;

  errno = 0;
  fd = open(append_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
  expect_ok("open_WRONLY_CREAT_TRUNC", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("write_append_first", write_all(fd, "abc", 3));
    errno = 0;
    expect_ok("fsync_append_file", fsync(fd));
    close(fd);
  }

  errno = 0;
  fd = open(append_path, O_WRONLY | O_APPEND);
  expect_ok("open_append", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("write_append_second", write_all(fd, "XYZ", 3));
    close(fd);
  }
  errno = 0;
  report("readback_append_exact",
         read_exact_file(append_path, buffer, 6) == 0 &&
             memcmp(buffer, "abcXYZ", 6) == 0);

  errno = 0;
  fd = open(excl_path, O_WRONLY | O_CREAT | O_EXCL, 0644);
  expect_ok("open_CREAT_EXCL_new", fd);
  if (fd >= 0) {
    close(fd);
  }
  errno = 0;
  fd = open(excl_path, O_WRONLY | O_CREAT | O_EXCL, 0644);
  expect_errno("open_CREAT_EXCL_existing", fd, EEXIST);
  if (fd >= 0) {
    close(fd);
  }

  errno = 0;
  fd = open(rw_path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  expect_ok("open_RDWR_create", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("write_read_file", write_all(fd, "native", 6));
    errno = 0;
    expect_ok("seek_start", lseek(fd, 0, SEEK_SET));
    memset(buffer, 0, sizeof(buffer));
    errno = 0;
    report("read_same_fd_exact", read(fd, buffer, 6) == 6 &&
                                      memcmp(buffer, "native", 6) == 0);
    close(fd);
  }
}

static void directory_and_link_cases(void) {
  struct stat st;
  char link_buffer[256] = {0};
  ssize_t link_len;

  errno = 0;
  expect_ok("mkdir_dir", mkdir(path_for("dir"), 0755));
  errno = 0;
  expect_errno("mkdir_dir_EEXIST", mkdir(path_for("dir"), 0755), EEXIST);
  errno = 0;
  expect_ok("rmdir_dir", rmdir(path_for("dir")));

  write_text_file(path_for("rename_src.txt"), "src", 0644);
  write_text_file(path_for("rename_dst.txt"), "dst", 0644);
  errno = 0;
  expect_ok("rename_overwrite", rename(path_for("rename_src.txt"),
                                       path_for("rename_dst.txt")));
  memset(link_buffer, 0, sizeof(link_buffer));
  report("rename_readback_exact",
         read_exact_file(path_for("rename_dst.txt"), link_buffer, 3) == 0 &&
             memcmp(link_buffer, "src", 3) == 0);

  errno = 0;
  expect_ok("symlink_relative", symlink("rename_dst.txt", path_for("link.txt")));
  errno = 0;
  expect_ok("lstat_symlink", lstat(path_for("link.txt"), &st));
  report("lstat_symlink_type", (st.st_mode & S_IFMT) == S_IFLNK);
  errno = 0;
  link_len = readlink(path_for("link.txt"), link_buffer, sizeof(link_buffer) - 1);
  if (link_len >= 0) {
    link_buffer[link_len] = '\0';
  }
  report("readlink_relative_exact",
         link_len == (ssize_t)strlen("rename_dst.txt") &&
             strcmp(link_buffer, "rename_dst.txt") == 0);
}

static void fd_and_metadata_cases(void) {
  const char *trunc_path = path_for("trunc.txt");
  struct timespec ts[2] = {
      {.tv_sec = 1700000000, .tv_nsec = 123000000},
      {.tv_sec = 1700000001, .tv_nsec = 456000000},
  };
  int fd;
  int fd2;

  write_text_file(trunc_path, "truncate-me", 0644);
  errno = 0;
  expect_ok("truncate_4", truncate(trunc_path, 4));
  mode_line("stat_after_truncate_4", trunc_path, 0644, 4, S_IFREG);

  errno = 0;
  fd = open(trunc_path, O_RDWR);
  expect_ok("open_trunc_RDWR", fd);
  if (fd >= 0) {
    errno = 0;
    expect_ok("ftruncate_2", ftruncate(fd, 2));
    fmode_line("fstat_after_ftruncate_2", fd, 0644, 2, S_IFREG);

    errno = 0;
    fd2 = dup(fd);
    expect_ok("dup_fd", fd2);
    if (fd2 >= 0) {
      close(fd2);
    }

    errno = 0;
    fd2 = dup2(fd, 42);
    expect_ok("dup2_fd_42", fd2);
    if (fd2 >= 0) {
      close(fd2);
    }

    errno = 0;
    int flags = fcntl(fd, F_GETFL);
    expect_ok("fcntl_GETFL", flags);
    if (flags >= 0) {
      errno = 0;
      expect_ok("fcntl_SETFL_APPEND", fcntl(fd, F_SETFL, flags | O_APPEND));
      errno = 0;
      flags = fcntl(fd, F_GETFL);
      report("fcntl_APPEND_visible", flags >= 0 && (flags & O_APPEND) != 0);
    }
    close(fd);
  }

  errno = 0;
  expect_ok("utimensat_set", utimensat(AT_FDCWD, trunc_path, ts, 0));
}

static void memory_growth_write_case(void) {
  const size_t chunk_size = 64 * 1024;
  const size_t chunks = 100;
  const size_t allocation_size = 24 * 1024 * 1024;
  const char *large_path = path_for("large.bin");
  unsigned char *allocation = (unsigned char *)malloc(allocation_size);
  int fd;
  struct stat st;

  report("malloc_memory_growth_buffer", allocation != NULL);
  if (allocation == NULL) {
    return;
  }
  for (size_t i = 0; i < allocation_size; i++) {
    allocation[i] = (unsigned char)(i * 31u + 7u);
  }
  unsigned char *chunk = allocation + allocation_size - chunk_size;

  errno = 0;
  fd = open(large_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
  expect_ok("large_open_create", fd);
  if (fd >= 0) {
    int ok = 1;
    for (size_t i = 0; i < chunks; i++) {
      if (write_all(fd, chunk, chunk_size) != 0) {
        ok = 0;
        break;
      }
    }
    printf("%-32s %s errno=%d bytes=%llu\n", "large_write_after_growth",
           ok ? "ok" : "FAIL", ok ? 0 : errno,
           (unsigned long long)(ok ? chunk_size * chunks : 0));
    if (!ok) {
      failures++;
    }
    close(fd);
  }

  errno = 0;
  if (stat(large_path, &st) == 0) {
    report("large_stat_size", st.st_size == (off_t)(chunk_size * chunks));
  } else {
    printf("%-32s FAIL errno=%d\n", "large_stat_size", errno);
    failures++;
  }

  char verify[64];
  errno = 0;
  report("large_readback_prefix",
         read_exact_file(large_path, verify, sizeof(verify)) == 0 &&
             memcmp(verify, chunk, sizeof(verify)) == 0);
  free(allocation);
}

int main(int argc, char **argv) {
  scratch_dir = argc > 1 ? argv[1] : "/tmp/fs-probe";
#if !defined(__wasi__) && !defined(__wasm32__)
  umask(0);
#endif
  mkdir(scratch_dir, 0777);
  cleanup();

  stat_and_access_cases();
  create_write_cases();
  directory_and_link_cases();
  fd_and_metadata_cases();
  memory_growth_write_case();

  cleanup();
  printf("SUMMARY failures=%u\n", failures);
  return failures == 0 ? 0 : 1;
}
