/* Test whether a basic vsprintf invocation works. */

#include <stdarg.h>
#include <stdio.h>

#include "../basic.h"

static int indirect(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	char buffer[64];
	int ret = vsprintf(buffer, format, ap);
	if ( ret < 0 )
		err(1, "vsprintf");
	if ( strlen(buffer) != (size_t) ret )
		err(1, "sprintf returned wrong length");
	const char* expected = "hello world 42";
	if ( strcmp(buffer, expected) != 0 )
		err(1, "vsprintf gave '%s' instead of '%s'", buffer, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}
