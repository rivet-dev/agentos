/* Test whether a basic memcmp invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char a[8] = "abcd\0fg";
	char b[8] = "abcd\0FG";
	int comparison = memcmp(a, b, 8);
	if ( comparison <= 0 )
		errx(1, "memcmp gave %d instead of non-negative", comparison);
	return 0;
}
