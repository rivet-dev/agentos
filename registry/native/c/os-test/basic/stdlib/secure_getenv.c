/* Test whether a basic secure_getenv invocation works. */

#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( setenv("FOO", "bar", 1) < 0 )
		err(1, "setenv");
	const char* value = secure_getenv("FOO");
	if ( !value )
	{
		// "Additional implementation-defined security criteria." so never an
		// error to end up here.
		return 0;
	}
	if ( geteuid() != getuid() )
		errx(1, "non-null but geteuid() != getuid()");
	if ( getegid() != getgid() )
		errx(1, "non-null but getegid() != getgid()");
	if ( strcmp(value, "bar") != 0 )
		errx(1, "secure_getenv(\"FOO\") was \"%s\", not \"bar\"", value);
	return 0;
}
