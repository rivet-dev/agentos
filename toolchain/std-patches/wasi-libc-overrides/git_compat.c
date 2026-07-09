#include <errno.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <limits.h>
#include <netdb.h>
#include <pwd.h>
#include <stdint.h>
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

#define WASM_IMPORT(mod, fn) \
	__attribute__((__import_module__(mod), __import_name__(fn)))

WASM_IMPORT("host_process", "proc_itimer_real")
uint32_t __host_proc_itimer_real(uint32_t operation, int64_t value_us,
	int64_t interval_us, uint64_t *ret_remaining_us,
	uint64_t *ret_interval_us);

static int timeval_to_microseconds(const struct timeval *value,
	int64_t *microseconds) {
	if (value->tv_sec < 0 || value->tv_usec < 0 || value->tv_usec >= 1000000 ||
		(uint64_t)value->tv_sec >
			((uint64_t)INT64_MAX - (uint64_t)value->tv_usec) / 1000000) {
		errno = EINVAL;
		return -1;
	}
	*microseconds = (int64_t)value->tv_sec * 1000000 + value->tv_usec;
	return 0;
}

static void microseconds_to_timeval(uint64_t microseconds,
	struct timeval *value) {
	value->tv_sec = (time_t)(microseconds / 1000000);
	value->tv_usec = (suseconds_t)(microseconds % 1000000);
}

int getitimer(int which, struct itimerval *value) {
	if (value == NULL) {
		errno = EFAULT;
		return -1;
	}
	if (which == ITIMER_VIRTUAL || which == ITIMER_PROF) {
		/*
		 * A disabled CPU timer has no delivery/instrumentation requirement.
		 * Match Linux for the observable zero state instead of rejecting the
		 * selector merely because active CPU-time delivery is unavailable.
		 */
		memset(value, 0, sizeof(*value));
		return 0;
	}
	if (which != ITIMER_REAL) {
		errno = EINVAL;
		return -1;
	}
	uint64_t remaining_us = 0;
	uint64_t interval_us = 0;
	uint32_t error = __host_proc_itimer_real(0, 0, 0,
		&remaining_us, &interval_us);
	if (error != 0) {
		errno = (int)error;
		return -1;
	}
	microseconds_to_timeval(remaining_us, &value->it_value);
	microseconds_to_timeval(interval_us, &value->it_interval);
	return 0;
}

int setitimer(int which, const struct itimerval *new_value,
	struct itimerval *old_value) {
	if (which != ITIMER_REAL && which != ITIMER_VIRTUAL &&
		which != ITIMER_PROF) {
		errno = EINVAL;
		return -1;
	}
	int64_t value_us = 0;
	int64_t interval_us = 0;
	if (new_value != NULL &&
		(timeval_to_microseconds(&new_value->it_value, &value_us) != 0 ||
		 timeval_to_microseconds(&new_value->it_interval, &interval_us) != 0))
		return -1;
	if (which == ITIMER_VIRTUAL || which == ITIMER_PROF) {
		if (value_us != 0 || interval_us != 0) {
			errno = ENOTSUP;
			return -1;
		}
		if (old_value != NULL)
			memset(old_value, 0, sizeof(*old_value));
		return 0;
	}
	uint64_t old_remaining_us = 0;
	uint64_t old_interval_us = 0;
	uint32_t error = __host_proc_itimer_real(1, value_us, interval_us,
		&old_remaining_us, &old_interval_us);
	if (error != 0) {
		errno = (int)error;
		return -1;
	}
	if (old_value != NULL) {
		microseconds_to_timeval(old_remaining_us, &old_value->it_value);
		microseconds_to_timeval(old_interval_us, &old_value->it_interval);
	}
	return 0;
}

unsigned alarm(unsigned seconds) {
	struct itimerval next;
	struct itimerval previous;
	memset(&next, 0, sizeof(next));
	next.it_value.tv_sec = seconds;
	if (setitimer(ITIMER_REAL, &next, &previous) != 0)
		return 0;
	uint64_t remaining = (uint64_t)previous.it_value.tv_sec;
	if (previous.it_value.tv_usec != 0)
		remaining++;
	return remaining > UINT_MAX ? UINT_MAX : (unsigned)remaining;
}

WASM_IMPORT("host_process", "proc_getpgid")
uint32_t __host_proc_getpgid(uint32_t pid, uint32_t *ret_pgid);

WASM_IMPORT("host_process", "proc_setpgid")
uint32_t __host_proc_setpgid(uint32_t pid, uint32_t pgid);

pid_t getpgid(pid_t pid) {
	uint32_t pgid = 0;
	uint32_t error = __host_proc_getpgid((uint32_t)pid, &pgid);
	if (error != 0) {
		errno = (int)error;
		return -1;
	}
	return (pid_t)pgid;
}

int setpgid(pid_t pid, pid_t pgid) {
	uint32_t error = __host_proc_setpgid((uint32_t)pid, (uint32_t)pgid);
	if (error != 0) {
		errno = (int)error;
		return -1;
	}
	return 0;
}

WASM_IMPORT("host_process", "proc_exec")
uint32_t __host_proc_exec(
	const uint8_t *exec_path_ptr, uint32_t exec_path_len,
	const uint8_t *argv_ptr, uint32_t argv_len,
	const uint8_t *envp_ptr, uint32_t envp_len,
	const uint32_t *cloexec_fds_ptr, uint32_t cloexec_fds_len);

WASM_IMPORT("host_process", "proc_fexec")
uint32_t __host_proc_fexec(
	uint32_t exec_fd,
	const uint8_t *argv_ptr, uint32_t argv_len,
	const uint8_t *envp_ptr, uint32_t envp_len,
	const uint32_t *cloexec_fds_ptr, uint32_t cloexec_fds_len);

uint32_t __agentos_copy_cloexec_fds(uint32_t *out, uint32_t capacity);

static int exec_host(const char *path, char *const argv[], char *const envp[]) {
	size_t argv_len = 0, env_len = 0;
	size_t path_len = strlen(path);
	if (path_len > UINT32_MAX) { errno = E2BIG; return -1; }
	for (size_t i = 0; argv && argv[i]; i++) {
		size_t len = strlen(argv[i]) + 1;
		if (len > UINT32_MAX - argv_len) { errno = E2BIG; return -1; }
		argv_len += len;
	}
	for (size_t i = 0; envp && envp[i]; i++) {
		size_t len = strlen(envp[i]) + 1;
		if (len > UINT32_MAX - env_len) { errno = E2BIG; return -1; }
		env_len += len;
	}
	uint8_t *argv_buf = argv_len ? malloc(argv_len) : NULL;
	uint8_t *env_buf = env_len ? malloc(env_len) : NULL;
	if ((argv_len && !argv_buf) || (env_len && !env_buf)) {
		free(argv_buf);
		free(env_buf);
		errno = ENOMEM;
		return -1;
	}
	uint8_t *cursor = argv_buf;
	for (size_t i = 0; argv && argv[i]; i++) {
		size_t len = strlen(argv[i]) + 1;
		memcpy(cursor, argv[i], len);
		cursor += len;
	}
	cursor = env_buf;
	for (size_t i = 0; envp && envp[i]; i++) {
		size_t len = strlen(envp[i]) + 1;
		memcpy(cursor, envp[i], len);
		cursor += len;
	}
	uint32_t cloexec_count = __agentos_copy_cloexec_fds(NULL, 0);
	uint32_t *cloexec_fds = cloexec_count ? malloc(sizeof(uint32_t) * cloexec_count) : NULL;
	if (cloexec_count && !cloexec_fds) {
		free(argv_buf);
		free(env_buf);
		errno = ENOMEM;
		return -1;
	}
	if (cloexec_count) {
		uint32_t actual = __agentos_copy_cloexec_fds(cloexec_fds, cloexec_count);
		if (actual > cloexec_count) {
			free(argv_buf);
			free(env_buf);
			free(cloexec_fds);
			errno = EAGAIN;
			return -1;
		}
		cloexec_count = actual;
	}

	uint32_t err = __host_proc_exec(
		(const uint8_t *)path, (uint32_t)path_len,
		argv_buf ? argv_buf : (const uint8_t *)"", (uint32_t)argv_len,
		env_buf ? env_buf : (const uint8_t *)"", (uint32_t)env_len,
		cloexec_fds, cloexec_count);
	free(argv_buf);
	free(env_buf);
	free(cloexec_fds);
	errno = err ? (int)err : EIO;
	return -1;
}

int fexecve(int fd, char *const argv[], char *const envp[]) {
	size_t argv_len = 0, env_len = 0;
	for (size_t i = 0; argv && argv[i]; i++) {
		size_t len = strlen(argv[i]) + 1;
		if (len > UINT32_MAX - argv_len) { errno = E2BIG; return -1; }
		argv_len += len;
	}
	for (size_t i = 0; envp && envp[i]; i++) {
		size_t len = strlen(envp[i]) + 1;
		if (len > UINT32_MAX - env_len) { errno = E2BIG; return -1; }
		env_len += len;
	}
	uint8_t *argv_buf = argv_len ? malloc(argv_len) : NULL;
	uint8_t *env_buf = env_len ? malloc(env_len) : NULL;
	if ((argv_len && !argv_buf) || (env_len && !env_buf)) {
		free(argv_buf);
		free(env_buf);
		errno = ENOMEM;
		return -1;
	}
	uint8_t *cursor = argv_buf;
	for (size_t i = 0; argv && argv[i]; i++) {
		size_t len = strlen(argv[i]) + 1;
		memcpy(cursor, argv[i], len);
		cursor += len;
	}
	cursor = env_buf;
	for (size_t i = 0; envp && envp[i]; i++) {
		size_t len = strlen(envp[i]) + 1;
		memcpy(cursor, envp[i], len);
		cursor += len;
	}
	uint32_t cloexec_count = __agentos_copy_cloexec_fds(NULL, 0);
	uint32_t *cloexec_fds = cloexec_count ? malloc(sizeof(uint32_t) * cloexec_count) : NULL;
	if (cloexec_count && !cloexec_fds) {
		free(argv_buf);
		free(env_buf);
		errno = ENOMEM;
		return -1;
	}
	if (cloexec_count) {
		uint32_t actual = __agentos_copy_cloexec_fds(cloexec_fds, cloexec_count);
		if (actual > cloexec_count) {
			free(argv_buf);
			free(env_buf);
			free(cloexec_fds);
			errno = EAGAIN;
			return -1;
		}
		cloexec_count = actual;
	}

	uint32_t err = __host_proc_fexec(
		(uint32_t)fd,
		argv_buf ? argv_buf : (const uint8_t *)"", (uint32_t)argv_len,
		env_buf ? env_buf : (const uint8_t *)"", (uint32_t)env_len,
		cloexec_fds, cloexec_count);
	free(argv_buf);
	free(env_buf);
	free(cloexec_fds);
	errno = err ? (int)err : EIO;
	return -1;
}

static int exec_shell_fallback(const char *path, char *const argv[], char *const envp[]) {
	size_t argc = 0;
	while (argv && argv[argc]) argc++;
	if (argc > SIZE_MAX / sizeof(char *) - 2) {
		errno = E2BIG;
		return -1;
	}
	char **shell_argv = calloc(argc + 3, sizeof(*shell_argv));
	if (!shell_argv) {
		errno = ENOMEM;
		return -1;
	}
	shell_argv[0] = (char *)"/bin/sh";
	shell_argv[1] = (char *)path;
	for (size_t i = 1; i < argc; i++) shell_argv[i + 1] = argv[i];
	int result = exec_host("/bin/sh", shell_argv, envp);
	free(shell_argv);
	return result;
}

static int exec_spawn_wait(const char *path, char *const argv[], char *const envp[], int search_path) {
	if (!path || path[0] == '\0') {
		errno = ENOENT;
		return -1;
	}
	if (!search_path || strchr(path, '/')) {
		if (strchr(path, '/')) {
			int result = exec_host(path, argv, envp);
			if (search_path && errno == ENOEXEC)
				return exec_shell_fallback(path, argv, envp);
			return result;
		}
		size_t len = strlen(path);
		char *relative = malloc(len + 3);
		if (!relative) { errno = ENOMEM; return -1; }
		relative[0] = '.';
		relative[1] = '/';
		memcpy(relative + 2, path, len + 1);
		int result = exec_host(relative, argv, envp);
		free(relative);
		return result;
	}

	const char *search = getenv("PATH");
	if (!search) search = "/bin:/usr/bin";
	int saw_eacces = 0;
	const char *segment = search;
	for (;;) {
		const char *colon = strchr(segment, ':');
		size_t dir_len = colon ? (size_t)(colon - segment) : strlen(segment);
		size_t file_len = strlen(path);
		size_t candidate_len = dir_len ? dir_len + 1 + file_len : file_len + 2;
		char *candidate = malloc(candidate_len + 1);
		if (!candidate) { errno = ENOMEM; return -1; }
		if (dir_len) {
			memcpy(candidate, segment, dir_len);
			candidate[dir_len] = '/';
			memcpy(candidate + dir_len + 1, path, file_len + 1);
		} else {
			candidate[0] = '.';
			candidate[1] = '/';
			memcpy(candidate + 2, path, file_len + 1);
		}
		exec_host(candidate, argv, envp);
		if (errno == ENOEXEC) {
			int result = exec_shell_fallback(candidate, argv, envp);
			free(candidate);
			return result;
		}
		free(candidate);
		if (errno == EACCES) saw_eacces = 1;
		else if (errno != ENOENT && errno != ENOTDIR) return -1;
		if (!colon) break;
		segment = colon + 1;
	}
	errno = saw_eacces ? EACCES : ENOENT;
	return -1;
}

static int exec_list(const char *path, const char *arg, va_list ap, int search_path) {
	extern char **environ;
	va_list count_ap;
	size_t argc = arg ? 1 : 0;
	char **argv;
	const char *next;

	if (arg) {
		va_copy(count_ap, ap);
		while ((next = va_arg(count_ap, const char *)) != NULL)
			argc++;
		va_end(count_ap);
	}

	argv = calloc(argc + 1, sizeof(*argv));
	if (!argv) {
		errno = ENOMEM;
		return -1;
	}

	if (arg) argv[0] = (char *)arg;
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

int execle(const char *path, const char *arg, ...) {
	va_list ap, count_ap;
	va_start(ap, arg);
	va_copy(count_ap, ap);
	size_t argc = arg ? 1 : 0;
	if (arg) while (va_arg(count_ap, const char *) != NULL) argc++;
	char *const *envp = va_arg(count_ap, char *const *);
	va_end(count_ap);

	char **argv = calloc(argc + 1, sizeof(*argv));
	if (!argv) {
		va_end(ap);
		errno = ENOMEM;
		return -1;
	}
	if (arg) argv[0] = (char *)arg;
	for (size_t i = 1; i < argc; i++) argv[i] = va_arg(ap, char *);
	if (arg) (void)va_arg(ap, char *);
	(void)va_arg(ap, char **);
	va_end(ap);
	int result = exec_spawn_wait(path, argv, envp, 0);
	free(argv);
	return result;
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

int execvpe(const char *file, char *const argv[], char *const envp[]) {
	return exec_spawn_wait(file, argv, envp, 1);
}

_Thread_local int h_errno;

int *__h_errno_location(void) {
	return &h_errno;
}

const char *hstrerror(int err) {
	switch (err) {
	case 0:
		return "Resolver Error 0 (no error)";
	case HOST_NOT_FOUND:
		return "Unknown host";
	case TRY_AGAIN:
		return "Host name lookup failure";
	case NO_RECOVERY:
		return "Unknown server error";
#ifdef NO_DATA
	case NO_DATA:
		return "No address associated with name";
#endif
	default:
		return "Unknown resolver error";
	}
}

/* The host resolver and live /etc/hosts reader are bounded independently.
 * Keep this legacy static-result API bounded too, and fail the whole lookup
 * instead of returning a silently incomplete address list. */
#define AGENTOS_GETHOSTBYNAME_MAX_ADDRESSES 64

struct hostent *gethostbyname(const char *name) {
	static _Thread_local struct hostent entry;
	static _Thread_local char *canonical;
	static _Thread_local size_t canonical_capacity;
	static _Thread_local unsigned char (*addresses)[sizeof(struct in_addr)];
	static _Thread_local size_t address_capacity;
	static _Thread_local char **address_list;
	static _Thread_local char *aliases[1];
	struct addrinfo hints, *results = NULL, *cursor;
	const char *canonical_source = NULL;
	unsigned char (*new_addresses)[sizeof(struct in_addr)];
	char **new_address_list, *new_canonical;
	size_t count = 0, index = 0, required;
	int error;

	if (name == NULL || *name == '\0') {
		h_errno = HOST_NOT_FOUND;
		return NULL;
	}
	memset(&hints, 0, sizeof(hints));
	hints.ai_family = AF_INET;
	hints.ai_socktype = SOCK_STREAM;
	hints.ai_flags = AI_CANONNAME;
	error = getaddrinfo(name, NULL, &hints, &results);
	if (error != 0) {
		switch (error) {
		case EAI_AGAIN:
			h_errno = TRY_AGAIN;
			break;
		case EAI_FAIL:
			h_errno = NO_RECOVERY;
			break;
		case EAI_NONAME:
#ifdef EAI_NODATA
		case EAI_NODATA:
#endif
			h_errno = HOST_NOT_FOUND;
			break;
		default:
			h_errno = NO_RECOVERY;
			break;
		}
		return NULL;
	}
	for (cursor = results; cursor != NULL; cursor = cursor->ai_next) {
		struct sockaddr_in *address;
		if (cursor->ai_family != AF_INET || cursor->ai_addr == NULL ||
		    cursor->ai_addrlen < (socklen_t)sizeof(*address))
			continue;
		if (canonical_source == NULL && cursor->ai_canonname != NULL)
			canonical_source = cursor->ai_canonname;
		count++;
	}
	if (count == 0) {
		freeaddrinfo(results);
		h_errno = NO_DATA;
		return NULL;
	}
	if (count > AGENTOS_GETHOSTBYNAME_MAX_ADDRESSES) {
		freeaddrinfo(results);
		errno = ERANGE;
		h_errno = NO_RECOVERY;
		return NULL;
	}
	if (count > SIZE_MAX / sizeof(*addresses) ||
	    count > SIZE_MAX / sizeof(*address_list) - 1) {
		freeaddrinfo(results);
		errno = EOVERFLOW;
		h_errno = NO_RECOVERY;
		return NULL;
	}
	if (count > address_capacity) {
		new_addresses = realloc(addresses, count * sizeof(*addresses));
		if (new_addresses == NULL) {
			freeaddrinfo(results);
			errno = ENOMEM;
			h_errno = NO_RECOVERY;
			return NULL;
		}
		addresses = new_addresses;
		new_address_list = realloc(address_list,
		    (count + 1) * sizeof(*address_list));
		if (new_address_list == NULL) {
			freeaddrinfo(results);
			errno = ENOMEM;
			h_errno = NO_RECOVERY;
			return NULL;
		}
		address_list = new_address_list;
		address_capacity = count;
	}
	if (canonical_source == NULL)
		canonical_source = name;
	required = strlen(canonical_source) + 1;
	if (required > canonical_capacity) {
		new_canonical = realloc(canonical, required);
		if (new_canonical == NULL) {
			freeaddrinfo(results);
			errno = ENOMEM;
			h_errno = NO_RECOVERY;
			return NULL;
		}
		canonical = new_canonical;
		canonical_capacity = required;
	}
	memcpy(canonical, canonical_source, required);
	for (cursor = results; cursor != NULL; cursor = cursor->ai_next) {
		struct sockaddr_in *address;

		if (cursor->ai_family != AF_INET || cursor->ai_addr == NULL ||
		    cursor->ai_addrlen < (socklen_t)sizeof(*address))
			continue;
		address = (struct sockaddr_in *)cursor->ai_addr;
		memcpy(addresses[index], &address->sin_addr,
		    sizeof(address->sin_addr));
		address_list[index] = (char *)addresses[index];
		index++;
	}
	freeaddrinfo(results);
	address_list[count] = NULL;
	aliases[0] = NULL;
	entry.h_name = canonical;
	entry.h_aliases = aliases;
	entry.h_addrtype = AF_INET;
	entry.h_length = sizeof(struct in_addr);
	entry.h_addr_list = address_list;
	h_errno = 0;
	return &entry;
}

static int close_database(FILE *database, const char *path, int primary_error) {
	if (fclose(database) != 0) {
		int close_error = errno != 0 ? errno : EIO;
		if (primary_error != 0) {
			fprintf(stderr, "%s: close failed after error %s: %s\n", path,
			    strerror(primary_error), strerror(close_error));
		} else {
			primary_error = close_error;
		}
	}
	if (primary_error != 0) {
		errno = primary_error;
		return -1;
	}
	return 0;
}

struct servent *getservbyname(const char *name, const char *proto) {
	static _Thread_local struct servent entry;
	static _Thread_local char line[1024];
	static _Thread_local char *aliases[64];
	char *comment, *save, *canonical, *port_proto, *slash, *alias, *end;
	FILE *services;
	unsigned long port;
	int matched, alias_count;

	if (name == NULL)
		return NULL;
	services = fopen("/etc/services", "r");
	if (services == NULL)
		return NULL;
	while (fgets(line, sizeof(line), services) != NULL) {
		if (strchr(line, '\n') == NULL && !feof(services)) {
			int ch;
			while ((ch = fgetc(services)) != '\n' && ch != EOF)
				;
			(void)close_database(services, "/etc/services", ERANGE);
			return NULL;
		}
		if ((comment = strchr(line, '#')) != NULL)
			*comment = '\0';
		save = NULL;
		canonical = strtok_r(line, " \t\r\n", &save);
		port_proto = strtok_r(NULL, " \t\r\n", &save);
		if (canonical == NULL || port_proto == NULL ||
		    (slash = strchr(port_proto, '/')) == NULL)
			continue;
		*slash++ = '\0';
		port = strtoul(port_proto, &end, 10);
		if (*port_proto == '\0' || *end != '\0' || port > 65535 ||
		    (proto != NULL && strcmp(proto, slash) != 0))
			continue;
		matched = strcmp(name, canonical) == 0;
		alias_count = 0;
		while ((alias = strtok_r(NULL, " \t\r\n", &save)) != NULL) {
			if (alias_count >= (int)(sizeof(aliases) / sizeof(aliases[0])) - 1) {
				(void)close_database(services, "/etc/services", ERANGE);
				return NULL;
			}
			aliases[alias_count++] = alias;
			if (strcmp(name, alias) == 0)
				matched = 1;
		}
		if (!matched)
			continue;
		aliases[alias_count] = NULL;
		entry.s_name = canonical;
		entry.s_aliases = aliases;
		entry.s_port = htons((unsigned short)port);
		entry.s_proto = slash;
		if (close_database(services, "/etc/services", 0) < 0)
			return NULL;
		return &entry;
	}
	if (ferror(services)) {
		int read_error = errno != 0 ? errno : EIO;
		(void)close_database(services, "/etc/services", read_error);
		return NULL;
	}
	(void)close_database(services, "/etc/services", 0);
	return NULL;
}

/* Retained for the historical projected-/etc account source. Current builds
 * use the bounded kernel account database from 0037-user-account-database.patch. */
#ifdef AGENTOS_WASI_LIBC_LEGACY_USER_SHIMS
struct passwd *getpwnam(const char *name) {
	static _Thread_local struct passwd entry;
	static _Thread_local char line[1024];
	char *fields[7], *cursor, *end;
	FILE *passwd;
	unsigned long uid, gid;
	int index;

	if (name == NULL)
		return NULL;
	passwd = fopen("/etc/passwd", "r");
	if (passwd == NULL)
		return NULL;
	while (fgets(line, sizeof(line), passwd) != NULL) {
		if (strchr(line, '\n') == NULL && !feof(passwd)) {
			int ch;
			while ((ch = fgetc(passwd)) != '\n' && ch != EOF)
				;
			(void)close_database(passwd, "/etc/passwd", ERANGE);
			return NULL;
		}
		line[strcspn(line, "\r\n")] = '\0';
		fields[0] = line;
		cursor = line;
		for (index = 1; index < 7; index++) {
			cursor = strchr(cursor, ':');
			if (cursor == NULL)
				break;
			*cursor++ = '\0';
			fields[index] = cursor;
		}
		if (index != 7 || strchr(fields[6], ':') != NULL ||
		    strcmp(fields[0], name) != 0)
			continue;
		uid = strtoul(fields[2], &end, 10);
		if (*fields[2] == '\0' || *end != '\0' || uid > UINT32_MAX)
			continue;
		gid = strtoul(fields[3], &end, 10);
		if (*fields[3] == '\0' || *end != '\0' || gid > UINT32_MAX)
			continue;
		entry.pw_name = fields[0];
		entry.pw_passwd = fields[1];
		entry.pw_uid = (uid_t)uid;
		entry.pw_gid = (gid_t)gid;
		entry.pw_gecos = fields[4];
		entry.pw_dir = fields[5];
		entry.pw_shell = fields[6];
		if (close_database(passwd, "/etc/passwd", 0) < 0)
			return NULL;
		return &entry;
	}
	if (ferror(passwd)) {
		int read_error = errno != 0 ? errno : EIO;
		(void)close_database(passwd, "/etc/passwd", read_error);
		return NULL;
	}
	(void)close_database(passwd, "/etc/passwd", 0);
	return NULL;
}
#endif

char *inet_ntoa(struct in_addr in) {
	static _Thread_local char buf[INET_ADDRSTRLEN];

	if (inet_ntop(AF_INET, &in, buf, sizeof(buf)) == NULL) {
		buf[0] = '0';
		buf[1] = '\0';
	}
	return buf;
}

static char syslog_ident[128];
static int syslog_option;
static int syslog_facility = LOG_USER;
static int syslog_mask = LOG_UPTO(LOG_DEBUG);

void openlog(const char *ident, int option, int facility) {
	if (ident != NULL)
		(void)snprintf(syslog_ident, sizeof(syslog_ident), "%s", ident);
	else
		syslog_ident[0] = '\0';
	syslog_option = option;
	if ((facility & ~LOG_FACMASK) == 0)
		syslog_facility = facility;
}

void syslog(int priority, const char *format, ...) {
	va_list ap;

	va_start(ap, format);
	vsyslog(priority, format, ap);
	va_end(ap);
}

void closelog(void) {
	syslog_ident[0] = '\0';
	syslog_option = 0;
}

int setlogmask(int mask) {
	int previous = syslog_mask;

	if (mask != 0)
		syslog_mask = mask;
	return previous;
}

void vsyslog(int priority, const char *format, va_list ap) {
	char message[2048], rendered[2304];
	int saved_errno = errno, length;
	size_t offset = 0;
	ssize_t written;

	if (format == NULL || (syslog_mask & LOG_MASK(LOG_PRI(priority))) == 0)
		return;
	if ((priority & LOG_FACMASK) == 0)
		priority |= syslog_facility;
	(void)priority;
	(void)vsnprintf(message, sizeof(message), format, ap);
	if (syslog_ident[0] != '\0' && (syslog_option & LOG_PID) != 0)
		length = snprintf(rendered, sizeof(rendered), "%s[%ld]: %s\n",
		    syslog_ident, (long)getpid(), message);
	else if (syslog_ident[0] != '\0')
		length = snprintf(rendered, sizeof(rendered), "%s: %s\n",
		    syslog_ident, message);
	else
		length = snprintf(rendered, sizeof(rendered), "%s\n", message);
	if (length < 0) {
		errno = saved_errno;
		return;
	}
	if ((size_t)length >= sizeof(rendered))
		length = sizeof(rendered) - 1;
	/* The VM has no syslog daemon endpoint. Route every accepted record to the
	 * host-visible stderr path instead of silently discarding diagnostics. */
	while (offset < (size_t)length) {
		written = write(STDERR_FILENO, rendered + offset,
		    (size_t)length - offset);
		if (written > 0) {
			offset += (size_t)written;
			continue;
		}
		if (written < 0 && errno == EINTR)
			continue;
		/* syslog() cannot return an error. Leave errno from the failed host-
		 * visible write intact so the failure is still observable. */
		return;
	}
	errno = saved_errno;
}
