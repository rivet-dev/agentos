/* Test whether a basic strcat invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char dst[8] = "foo";
	if ( strcat(dst, "bar") != dst )
		errx(1, "strcat did not return pointer to dst's end");
	const char* expected = "foobar";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "strcat gave %s not %s", dst, expected);
	return 0;
}
