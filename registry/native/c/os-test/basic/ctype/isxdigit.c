/* Test whether a basic isxdigit invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'f';
	char c2 = 'g';
	if ( !isxdigit(c1) )
		errx(1, "isxdigit('%c') was not true", c1);
	if ( isxdigit(c2) )
		errx(1, "isxdigit('%c') was not false", c2);
	return 0;
}
