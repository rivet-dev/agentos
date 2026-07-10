/**
 * mkstemp/mkdtemp family for AgentOS WASI.
 *
 * Upstream wasi-libc hides these because bare WASI has no ambient temp
 * directory. AgentOS supplies cwd-relative filesystem access and Rust std
 * already patches temp_dir(), so expose the POSIX APIs for C software too.
 */

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static const char alphabet[] =
	"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
static unsigned long counter;

static int fill_template(char *template, int suffix_len) {
	size_t len = strlen(template);
	if (len < (size_t)suffix_len + 6 ||
		memcmp(template + len - suffix_len - 6, "XXXXXX", 6) != 0) {
		errno = EINVAL;
		return -1;
	}

	uint64_t value = (uint64_t)time(NULL) ^ (uintptr_t)template ^ counter++;
	char *slot = template + len - suffix_len - 6;
	for (int i = 0; i < 6; i++) {
		value = value * 6364136223846793005ULL + 1442695040888963407ULL;
		slot[i] = alphabet[value % (sizeof(alphabet) - 1)];
	}
	return 0;
}

int mkostemps(char *template, int suffix_len, int flags) {
	char original[6];
	size_t len = strlen(template);
	if (len < (size_t)suffix_len + 6) {
		errno = EINVAL;
		return -1;
	}
	char *slot = template + len - suffix_len - 6;
	memcpy(original, slot, sizeof(original));

	flags &= ~O_ACCMODE;
	for (int retries = 100; retries > 0; retries--) {
		if (fill_template(template, suffix_len) != 0) {
			return -1;
		}
		int fd = open(template, flags | O_RDWR | O_CREAT | O_EXCL, 0600);
		if (fd >= 0) {
			return fd;
		}
		if (errno != EEXIST) {
			break;
		}
	}

	memcpy(slot, original, sizeof(original));
	return -1;
}

int mkstemps(char *template, int suffix_len) {
	return mkostemps(template, suffix_len, 0);
}

int mkostemp(char *template, int flags) {
	return mkostemps(template, 0, flags);
}

int mkstemp(char *template) {
	return mkostemps(template, 0, 0);
}

char *mktemp(char *template) {
	struct stat st;
	size_t len = strlen(template);
	if (len < 6 || memcmp(template + len - 6, "XXXXXX", 6) != 0) {
		errno = EINVAL;
		if (len > 0) {
			template[0] = '\0';
		}
		return template;
	}

	for (int retries = 100; retries > 0; retries--) {
		if (fill_template(template, 0) != 0) {
			template[0] = '\0';
			return template;
		}
		if (stat(template, &st) != 0) {
			if (errno != ENOENT) {
				template[0] = '\0';
			}
			return template;
		}
	}

	template[0] = '\0';
	errno = EEXIST;
	return template;
}

char *mkdtemp(char *template) {
	char original[6];
	size_t len = strlen(template);
	if (len < 6) {
		errno = EINVAL;
		return NULL;
	}
	char *slot = template + len - 6;
	memcpy(original, slot, sizeof(original));

	for (int retries = 100; retries > 0; retries--) {
		if (fill_template(template, 0) != 0) {
			return NULL;
		}
		if (mkdir(template, 0700) == 0) {
			return template;
		}
		if (errno != EEXIST) {
			break;
		}
	}

	memcpy(slot, original, sizeof(original));
	return NULL;
}
