/* Test whether a basic vprintf invocation works. */

#include <stdarg.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

static int indirect(char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( vprintf(format, ap) < 0 )
		err(1, "vprintf");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	char buf[256];
	size_t amount = fread(buf, 1, sizeof(buf) - 1, stdin);
	if ( ferror(stdin) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "hello world 42";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "vprintf wrote '%s' instead of '%s'", buf, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}

