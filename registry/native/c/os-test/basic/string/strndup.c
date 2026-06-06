/* Test whether a basic strndup invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char* src = "foo";
	char* dst = strndup(src, 2);
	if ( !dst )
		err(1, "malloc");
	const char* expected = "fo";
	if ( strcmp(expected, dst) != 0 )
		err(1, "strndup gave %s instead of %s", expected, dst);
	return 0;
}
