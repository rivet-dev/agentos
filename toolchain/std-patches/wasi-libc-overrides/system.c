#include <errno.h>
#include <stdio.h>
#include <stdlib.h>

int system(const char *command) {
	if (command == NULL) {
		return 0;
	}
	errno = ENOSYS;
	return -1;
}

FILE *popen(const char *command, const char *mode) {
	(void)command;
	(void)mode;
	errno = ENOSYS;
	return NULL;
}

int pclose(FILE *stream) {
	(void)stream;
	errno = ENOSYS;
	return -1;
}
