/* Test whether a basic unsetenv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	if ( setenv("FOO", "foo", 1) < 0 )
		err(1, "first setenv");
	const char* value = getenv("FOO");
	if ( !value )
		errx(1, "first getenv(\"FOO\") == NULL");
	if ( strcmp(value, "foo") != 0 )
		errx(1, "first getenv(\"FOO\") was \"%s\", not \"foo\"", value);
	if ( unsetenv("FOO") < 0 )
		err(1, "unsetenv");
	if ( getenv("FOO") )
		errx(1, "second getenv(\"FOO\") != NULL");
	return 0;
}
