/* Test whether a basic strncmp invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char a[8] = "abcdefg";
	char b[8] = "abcdeFG";
	int comparison = strncmp(a, b, 5);
	if ( comparison != 0 )
		errx(1, "strncmp gave %d instead of 0", comparison);
	return 0;
}
