#include <errno.h>
#include <err.h>
#include <signal.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <unistd.h>
#include <utmp.h>

/* Linux syscall(2) numbers have no meaning in a wasm32-wasip1 VM.  Returning
 * ENOSYS is the documented contract for an unimplemented syscall and lets
 * procps-ng's pidfd path fall back to the host-backed kill(2) implementation.
 * https://man7.org/linux/man-pages/man2/syscall.2.html */
long syscall(long number, ...) {
	(void)number;
	errno = ENOSYS;
	return -1;
}

/* getsid(2) is declared so portable consumers compile.  Session selection is
 * optional in procps-ng; until the host_process ABI exposes session lookup,
 * fail explicitly instead of inventing an id.
 * https://man7.org/linux/man-pages/man2/getsid.2.html */
pid_t getsid(pid_t pid) {
	(void)pid;
	errno = ENOSYS;
	return -1;
}

/* AgentOS' cooperative signal ABI currently transports a signal number, not
 * POSIX queued payload metadata.  Report that limit to callers exactly as
 * sigqueue(3) permits for an unsupported operation.
 * https://man7.org/linux/man-pages/man3/sigqueue.3.html */
int sigqueue(pid_t pid, int sig, union sigval value) {
	(void)pid;
	(void)sig;
	(void)value;
	errno = ENOSYS;
	return -1;
}

/* VMs do not maintain a host-login accounting database. getutent(3)'s normal
 * end-of-file result therefore represents the complete empty guest database;
 * set/end remain valid cursor lifecycle calls.
 * https://man7.org/linux/man-pages/man3/getutent.3.html */
void setutent(void) {
}

struct utmp *getutent(void) {
	return NULL;
}

void endutent(void) {
}

/* BSD err(3) is a libc compatibility API used by procps-ng. Keep diagnostics
 * on stderr and preserve errno only for the warn/err variants.
 * https://man7.org/linux/man-pages/man3/err.3.html */
void vwarn(const char *format, va_list args) {
	fprintf(stderr, "%s: ", program_invocation_short_name);
	if (format != NULL) {
		vfprintf(stderr, format, args);
		fputs(": ", stderr);
	}
	perror(NULL);
}

void vwarnx(const char *format, va_list args) {
	fprintf(stderr, "%s: ", program_invocation_short_name);
	if (format != NULL)
		vfprintf(stderr, format, args);
	fputc('\n', stderr);
}

void warn(const char *format, ...) {
	va_list args;
	va_start(args, format);
	vwarn(format, args);
	va_end(args);
}

void warnx(const char *format, ...) {
	va_list args;
	va_start(args, format);
	vwarnx(format, args);
	va_end(args);
}

_Noreturn void verr(int status, const char *format, va_list args) {
	vwarn(format, args);
	exit(status);
}

_Noreturn void verrx(int status, const char *format, va_list args) {
	vwarnx(format, args);
	exit(status);
}

_Noreturn void err(int status, const char *format, ...) {
	va_list args;
	va_start(args, format);
	verr(status, format, args);
}

_Noreturn void errx(int status, const char *format, ...) {
	va_list args;
	va_start(args, format);
	verrx(status, format, args);
}
