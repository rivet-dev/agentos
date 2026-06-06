/* Test whether a basic strcmp invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char a[8] = "abcdefg";
	char b[8] = "abcdeFG";
	int comparison = strcmp(a, b);
	if ( comparison <= 0 )
		errx(1, "strcmp gave %d instead of non-negative", comparison);
	return 0;
}
