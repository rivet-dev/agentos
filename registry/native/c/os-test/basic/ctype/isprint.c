/* Test whether a basic isprint invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'A';
	char c2 = '\r';
	if ( !isprint(c1) )
		errx(1, "isprint('%c') was not true", c1);
	if ( isprint(c2) )
		errx(1, "isprint('%c') was not false", c2);
	return 0;
}
