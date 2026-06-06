/* Test whether a basic snprintf invocation works. */

#include <stdio.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wformat-truncation"

int main(void)
{
	char buffer[10];
	int ret = snprintf(buffer, sizeof(buffer), "hello %s %d", "world", 42);
	if ( ret < 0 )
		err(1, "snprintf");
	if ( (size_t) ret != strlen("hello world 42") )
		err(1, "snprintf returned wrong length");
	const char* expected = "hello wor";
	if ( strcmp(buffer, expected) != 0 )
		err(1, "snprintf gave '%s' instead of '%s'", buffer, expected);
	return 0;
}
