/* Test whether a basic sprintf invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buffer[64];
	int ret = sprintf(buffer, "hello %s %d", "world", 42);
	if ( ret < 0 )
		err(1, "sprintf");
	if ( sizeof(buffer) <= (size_t) ret )
		errx(1, "sprintf buffer overrun, ret = %d", ret);
	if ( strlen(buffer) != (size_t) ret )
		err(1, "sprintf returned wrong length");
	const char* expected = "hello world 42";
	if ( strcmp(buffer, expected) != 0 )
		err(1, "sprintf gave '%s' instead of '%s'", buffer, expected);
	return 0;
}
