/* Test whether a basic strdup invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char* src = "foo";
	char* dst = strdup(src);
	if ( !dst )
		err(1, "malloc");
	if ( strcmp(src, dst) != 0 )
		err(1, "strdup gave %s instead of %s", src, dst);
	return 0;
}
