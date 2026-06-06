/* Test whether a basic vfprintf invocation works. */

#include <stdio.h>

#include "../basic.h"

static int indirect(char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( vfprintf(fp, format, ap) < 0 )
		err(1, "vfprintf");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	char buf[256];
	size_t amount = fread(buf, 1, sizeof(buf) - 1, fp);
	if ( ferror(fp) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "hello world 42";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "vfprintf wrote '%s' instead of '%s'", buf, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect("hello %s %d", "world", 42);
}
