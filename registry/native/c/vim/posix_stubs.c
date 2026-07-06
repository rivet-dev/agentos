/* posix_stubs.c — stubs for full-OS libc gaps vim references but that the VM
 * does not implement (group database, etc.). Safe no-op behavior. */
#include <grp.h>
#include <stddef.h>
struct group *getgrgid(gid_t g) { (void)g; return NULL; }
struct group *getgrnam(const char *n) { (void)n; return NULL; }
struct group *getgrent(void) { return NULL; }
void setgrent(void) {}
void endgrent(void) {}

/* --- additional full-OS libc gaps --- */
#include <sys/types.h>
mode_t umask(mode_t mask) { (void)mask; return 0; }

#include <sys/time.h>
struct itimerval;
int setitimer(int w, const struct itimerval *n, struct itimerval *o) { (void)w; (void)n; (void)o; return 0; }
int getitimer(int w, struct itimerval *o) { (void)w; (void)o; return 0; }

/* --- process/signal stubs (not used by core editing) --- */
#include <errno.h>
pid_t fork(void) { errno = ENOSYS; return -1; }
int execvp(const char *f, char *const a[]) { (void)f; (void)a; errno = ENOSYS; return -1; }
int raise(int s) { (void)s; return -1; }
/* signal sentinel symbols (this libc takes their address for SIG_IGN/SIG_ERR) */
void __SIG_IGN(int s) { (void)s; }
void __SIG_ERR(int s) { (void)s; }
void __SIG_DFL(int s) { (void)s; }
int sigprocmask(int how, const void *set, void *old) { (void)how; (void)set; (void)old; return 0; }
int sigpending(void *set) { (void)set; return 0; }
