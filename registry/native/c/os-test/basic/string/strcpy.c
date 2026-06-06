/* Test whether a basic strcpy invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";
	char* result = strcpy(dst, src);
	if ( result != dst )
		errx(1, "strcpy did not return dst");
	const char* expected = "abcdefg";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "strcpy gave %s instead of %s", dst, expected);
	return 0;
}
