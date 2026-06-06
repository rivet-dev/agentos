/* Test whether a basic strlcpy invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";
	size_t result = strlcpy(dst, src, 4);
	if ( result != 7 )
		errx(1, "strlcpy did not return attempted length");
	const char* expected = "abc";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "strlcpy gave %s instead of %s", dst, expected);
	return 0;
}
