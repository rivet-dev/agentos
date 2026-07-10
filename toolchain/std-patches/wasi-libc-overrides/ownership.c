/**
 * Minimal ownership and umask surface for POSIX software on AgentOS WASI.
 *
 * AgentOS tracks file mode bits, but does not currently model uid/gid ownership
 * mutation in the VFS. Expose the POSIX calls so upstream programs can build
 * unchanged. Ownership changes fail as unsupported at runtime; umask is a
 * process-local value used by programs that calculate extracted file modes.
 */

#include <errno.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static mode_t current_umask = 0022;

mode_t umask(mode_t mask) {
	mode_t previous = current_umask;
	current_umask = mask & 0777;
	return previous;
}

int chown(const char *path, uid_t owner, gid_t group) {
	(void)path;
	(void)owner;
	(void)group;
	errno = ENOSYS;
	return -1;
}

int fchown(int fd, uid_t owner, gid_t group) {
	(void)fd;
	(void)owner;
	(void)group;
	errno = ENOSYS;
	return -1;
}

int lchown(const char *path, uid_t owner, gid_t group) {
	(void)path;
	(void)owner;
	(void)group;
	errno = ENOSYS;
	return -1;
}

int fchownat(int fd, const char *path, uid_t owner, gid_t group, int flags) {
	(void)fd;
	(void)path;
	(void)owner;
	(void)group;
	(void)flags;
	errno = ENOSYS;
	return -1;
}
