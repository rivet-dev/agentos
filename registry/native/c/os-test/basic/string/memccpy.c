/*[XSI]*/
/* Test whether a basic memccpy invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8] = "ABCDEFG";

	// Try copying until after 'e'.
	void* result = memccpy(dst, src, 'e', 8);
	if ( !result )
		errx(1, "first memccpy returned NULL");
	if ( result != dst + 5 )
		errx(1, "first memccpy did not point to dst 'F'");
	const char* expected = "abcdeFG";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "first memccpy gave %s instead of %s", dst, expected);

	// Try copying until after 'x' (which does not occur).
	if ( memccpy(dst, src, 'x', 8) )
		errx(1, "second memccpy did not return NULL");
	expected = "abcdefg";
	if ( strcmp(dst, expected) != 0 )
		errx(1, "second memccpy gave %s instead of %s", dst, expected);

	return 0;
}
