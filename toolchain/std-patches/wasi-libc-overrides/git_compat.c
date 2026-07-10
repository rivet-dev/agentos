#include <errno.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <pwd.h>
#include <signal.h>
#include <stdarg.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <sys/types.h>
#include <syslog.h>
#include <unistd.h>

#ifdef h_errno
#undef h_errno
#endif

void __SIG_IGN(int sig) {
	(void)sig;
}

void __SIG_ERR(int sig) {
	(void)sig;
}

int raise(int sig) {
	return kill(getpid(), sig);
}

pid_t getpgid(pid_t pid) {
	return pid == 0 ? getpid() : pid;
}

unsigned alarm(unsigned seconds) {
	(void)seconds;
	return 0;
}

int getitimer(int which, struct itimerval *value) {
	(void)which;
	if (value)
		memset(value, 0, sizeof(*value));
	return 0;
}

int setitimer(int which, const struct itimerval *new_value, struct itimerval *old_value) {
	(void)which;
	(void)new_value;
	if (old_value)
		memset(old_value, 0, sizeof(*old_value));
	return 0;
}

static int exec_spawn_wait(const char *path, char *const argv[], char *const envp[], int search_path) {
	pid_t pid;
	int status;
	int err = search_path
		? posix_spawnp(&pid, path, NULL, NULL, argv, envp)
		: posix_spawn(&pid, path, NULL, NULL, argv, envp);

	if (err != 0) {
		errno = err;
		return -1;
	}

	while (waitpid(pid, &status, 0) < 0) {
		if (errno != EINTR)
			_exit(127);
	}

	if (WIFEXITED(status))
		_exit(WEXITSTATUS(status));
	if (WIFSIGNALED(status))
		_exit(128 + WTERMSIG(status));
	_exit(127);
}

static int exec_list(const char *path, const char *arg, va_list ap, int search_path) {
	extern char **environ;
	va_list count_ap;
	size_t argc = 1;
	char **argv;
	const char *next;

	va_copy(count_ap, ap);
	while ((next = va_arg(count_ap, const char *)) != NULL)
		argc++;
	va_end(count_ap);

	argv = calloc(argc + 1, sizeof(*argv));
	if (!argv) {
		errno = ENOMEM;
		return -1;
	}

	argv[0] = (char *)arg;
	for (size_t i = 1; i < argc; i++)
		argv[i] = va_arg(ap, char *);

	int ret = exec_spawn_wait(path, argv, environ, search_path);
	free(argv);
	return ret;
}

int execl(const char *path, const char *arg, ...) {
	va_list ap;
	va_start(ap, arg);
	int ret = exec_list(path, arg, ap, 0);
	va_end(ap);
	return ret;
}

int execlp(const char *file, const char *arg, ...) {
	va_list ap;
	va_start(ap, arg);
	int ret = exec_list(file, arg, ap, 1);
	va_end(ap);
	return ret;
}

int execv(const char *path, char *const argv[]) {
	extern char **environ;
	return exec_spawn_wait(path, argv, environ, 0);
}

int execve(const char *path, char *const argv[], char *const envp[]) {
	return exec_spawn_wait(path, argv, envp, 0);
}

int execvp(const char *file, char *const argv[]) {
	extern char **environ;
	return exec_spawn_wait(file, argv, environ, 1);
}

_Thread_local int h_errno;

int *__h_errno_location(void) {
	return &h_errno;
}

const char *hstrerror(int err) {
	(void)err;
	return strerror(errno);
}

struct hostent *gethostbyname(const char *name) {
	(void)name;
	h_errno = HOST_NOT_FOUND;
	return NULL;
}

struct servent *getservbyname(const char *name, const char *proto) {
	(void)name;
	(void)proto;
	return NULL;
}

struct passwd *getpwnam(const char *name) {
	(void)name;
	return NULL;
}

char *inet_ntoa(struct in_addr in) {
	static _Thread_local char buf[INET_ADDRSTRLEN];

	if (inet_ntop(AF_INET, &in, buf, sizeof(buf)) == NULL) {
		buf[0] = '0';
		buf[1] = '\0';
	}
	return buf;
}

void openlog(const char *ident, int option, int facility) {
	(void)ident;
	(void)option;
	(void)facility;
}

void syslog(int priority, const char *format, ...) {
	(void)priority;
	(void)format;
}

void closelog(void) {
}

int setlogmask(int mask) {
	return mask;
}

void vsyslog(int priority, const char *format, va_list ap) {
	(void)priority;
	(void)format;
	(void)ap;
}
