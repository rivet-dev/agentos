/* Test whether a basic stpncpy invocation works. */

#include <string.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";
	if ( stpncpy(dst, src, 4) != dst + 4 )
		errx(1, "stpncpy did not return pointer to dst's end");
	char expected[8] = "abcdEFG";
	if ( memcmp(dst, expected, 8) != 0 )
		errx(1, "stpncpy did not copy properly");
	return 0;
}
