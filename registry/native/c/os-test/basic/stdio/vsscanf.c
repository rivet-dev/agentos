/* Test whether a basic vsscanf invocation works. */

#include <stdarg.h>
#include <stdio.h>

#include "../basic.h"

static void indirect(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vsscanf("hello world 42", format, ap);
	if ( ret < 0 )
		err(1, "vsscanf");
	if ( ret != 2 )
		errx(1, "vsscanf did not return 2");
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
