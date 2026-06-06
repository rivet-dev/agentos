/* Test whether a basic memcpy invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";
	void* result = memcpy(dst, src, 3);
	if ( result != dst )
		errx(1, "memcpy did not return dst");
	const char* expected = "abcDEFG";
	if ( memcmp(dst, expected, 8) != 0 )
		errx(1, "memcpy gave %s instead of %s", dst, expected);
	return 0;
}
