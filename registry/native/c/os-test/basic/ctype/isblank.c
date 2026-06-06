/* Test whether a basic isblank invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = ' ';
	char c2 = '_';
	if ( !isblank(c1) )
		errx(1, "isblank('%c') was not true", c1);
	if ( isblank(c2) )
		errx(1, "isblank('%c') was not false", c2);
	return 0;
}
