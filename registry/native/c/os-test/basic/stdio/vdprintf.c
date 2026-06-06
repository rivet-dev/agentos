/* Test whether a basic vdprintf invocation works. */

#include <stdarg.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

static int indirect(char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	int ret = vdprintf(fds[1], format, ap);
	if ( ret < 0 )
		err(1, "vdprintf");
	const char* expected = "hello world 42";
	if ( (size_t) ret != strlen(expected) )
		errx(1, "vdprintf returned wrong length");
	char buffer[256];
	ssize_t amount = read(fds[0], buffer, sizeof(buffer) - 1);
	if ( amount < 0 )
		err(1, "read");
	buffer[amount] = 0;
	if ( strcmp(buffer, expected) != 0 )
		errx(1, "vdprintf wrote '%s' instead of '%s'", buffer, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}
