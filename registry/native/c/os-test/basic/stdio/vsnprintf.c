/* Test whether a basic vsnprintf invocation works. */

#include <stdarg.h>
#include <stdio.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wformat-truncation"

static int indirect(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	char buffer[10];
	int ret = vsnprintf(buffer, sizeof(buffer), format, ap);
	if ( ret < 0 )
		err(1, "vsnprintf");
	if ( (size_t) ret != strlen("hello world 42") )
		err(1, "vsnprintf returned wrong length");
	const char* expected = "hello wor";
	if ( strcmp(buffer, expected) != 0 )
		err(1, "vsnprintf gave '%s' instead of '%s'", buffer, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}
