/* Test whether a basic setenv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	const char* value;

	if ( setenv("FOO", "foo", 1) < 0 )
		err(1, "first setenv");
	if ( !(value = getenv("FOO")) )
		errx(1, "first getenv(\"FOO\") == NULL");
	if ( strcmp(value, "foo") != 0 )
		errx(1, "first getenv(\"FOO\") was \"%s\", not \"foo\"", value);

	if ( setenv("FOO", "bar", 0) < 0 )
		err(1, "second setenv");
	if ( !(value = getenv("FOO")) )
		errx(1, "second getenv(\"FOO\") == NULL");
	if ( strcmp(value, "foo") != 0 )
		errx(1, "second getenv(\"FOO\") was \"%s\", not \"foo\"", value);

	if ( setenv("FOO", "qux", 1) < 0 )
		err(1, "third setenv");
	if ( !(value = getenv("FOO")) )
		errx(1, "third getenv(\"FOO\") == NULL");
	if ( strcmp(value, "qux") != 0 )
		errx(1, "third getenv(\"FOO\") was \"%s\", not \"qux\"", value);

	return 0;
}
