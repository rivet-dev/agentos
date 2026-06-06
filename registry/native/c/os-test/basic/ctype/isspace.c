/* Test whether a basic isspace invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = ' ';
	char c2 = 'A';
	if ( !isspace(c1) )
		errx(1, "isspace('%c') was not true", c1);
	if ( isspace(c2) )
		errx(1, "isspace('%c') was not false", c2);
	return 0;
}
