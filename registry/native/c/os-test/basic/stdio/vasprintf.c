/* Test whether a basic vasprintf invocation works. */

#include <stdarg.h>
#include <stdio.h>

#include "../basic.h"

static int indirect(char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	char* buf;
	int ret = vasprintf(&buf, format, ap);
	if ( ret < 0 )
		err(1, "vasprintf");
	if ( strlen(buf) != (size_t) ret )
		errx(1, "vasprintf returned wrong length");
	const char* expected = "hello world 42";
	if ( strcmp(buf, expected) != 0 )
		err(1, "vasprintf gave '%s' instead of '%s'", buf, expected);
	free(buf);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}
