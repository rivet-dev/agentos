/* Test whether a basic strncpy invocation works. */

#include <string.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";
	if ( strncpy(dst, src, 4) != dst )
		errx(1, "strncpy did not return dst");
	char expected[8] = "abcdEFG";
	if ( memcmp(dst, expected, 8) != 0 )
		errx(1, "strncpy did not copy properly");
	return 0;
}
