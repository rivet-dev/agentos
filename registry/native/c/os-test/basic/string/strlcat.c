/* Test whether a basic strlcat invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "AB";
	size_t result = strlcat(dst, src, 8);
	if ( result != 9 )
		errx(1, "strlcat did not return attempted length");
	const char* expected = "ABabcde";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "strlcat gave %s instead of %s", dst, expected);
	return 0;
}
