/* Test whether a basic vscanf invocation works. */

#include <stdarg.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

static void indirect(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputs("hello world 42", stdout) == EOF )
		err(1, "puts");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	int ret = vscanf(format, ap);
	if ( ret < 0 )
		err(1, "vscanf");
	if ( ret != 2 )
		errx(1, "vscanf did not return 2");
	va_end(ap);
}

int main(void)
{
	char world[6];
	int value;
	indirect("hello %5s %d", world, &value);
	if ( strcmp(world, "world") != 0 )
		errx(1, "vsscanf gave '%s' instead of '%s'", world, "world");
	if ( value != 42 )
		errx(1, "vsscanf gave %d' instead of %d", value, 42);
	return 0;
}
