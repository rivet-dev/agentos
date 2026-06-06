/* Test whether a basic strncat invocation works. */

#include <string.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "AB";
	if ( strncat(dst, src, 4) != dst )
		errx(1, "strncat did not return dst");
	const char* expected = "ABabcd";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "strncat gave %s instead of %s", dst, expected);
	return 0;
}
