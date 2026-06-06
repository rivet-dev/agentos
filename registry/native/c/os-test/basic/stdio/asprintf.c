/* Test whether a basic asprintf invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char* buf;
	int ret = asprintf(&buf, "hello %s %d", "world", 42);
	if ( ret < 0 )
		err(1, "asprintf");
	if ( strlen(buf) != (size_t) ret )
		errx(1, "asprintf returned wrong length");
	const char* expected = "hello world 42";
	if ( strcmp(buf, expected) != 0 )
		err(1, "asprintf gave '%s' instead of '%s'", buf, expected);
	free(buf);
	return 0;
}
