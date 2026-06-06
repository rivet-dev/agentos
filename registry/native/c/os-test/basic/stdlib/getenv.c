/* Test whether a basic getenv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	if ( setenv("FOO", "bar", 1) < 0 )
		err(1, "setenv");
	const char* value = getenv("FOO");
	if ( !value )
		errx(1, "getenv(\"FOO\") == NULL");
	if ( strcmp(value, "bar") != 0 )
		errx(1, "getenv(\"FOO\") was \"%s\", not \"bar\"", value);
	return 0;
}
