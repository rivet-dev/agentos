/* Test whether a basic strncasecmp invocation works. */

#include <strings.h>

#include "../basic.h"

int main(void)
{
	if ( strncasecmp("foo", "FOX", 2) != 0 )
		errx(1, "strncasecmp(\"foo\", \"FOX\", 2) weren't equal");
	return 0;
}
