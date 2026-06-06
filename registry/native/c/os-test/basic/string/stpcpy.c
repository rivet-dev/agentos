/* Test whether a basic stpcpy invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char src[8] = "abcdefg";
	char dst[8];
	if ( stpcpy(dst, src) != dst + 7 )
		errx(1, "stpcpy did not return pointer to dst's end");
	if ( strcmp(src, dst) != 0 )
		errx(1, "stpcpy did not copy the string");
	return 0;
}
