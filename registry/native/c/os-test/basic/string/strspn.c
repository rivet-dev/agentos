/* Test whether a basic strspn invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "abcdefg";
	if ( strspn(buf, "abcdf") != 4 )
		errx(1, "strspn did not find 'e'");
	if ( strspn(buf, "abcdefg") != 7 )
		errx(1, "strspn found other character");
	return 0;
}
