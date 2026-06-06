/* Test whether a basic vfscanf invocation works. */

#include <stdarg.h>
#include <stdio.h>

#include "../basic.h"

static void indirect(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	char data[] = "hello world 42";
	FILE* fp = fmemopen(data, sizeof(data), "r");
	if ( !fp )
		err(1, "fmemopen");
	int ret = vfscanf(fp, format, ap);
	if ( ret < 0 )
		err(1, "vfscanf");
	if ( ret != 2 )
		errx(1, "vfscanf did not return 2");
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
