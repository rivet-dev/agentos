/* Test whether a basic iscntrl invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = '\r';
	char c2 = 'x';
	if ( !iscntrl(c1) )
		errx(1, "iscntrl('%c') was not true", c1);
	if ( iscntrl(c2) )
		errx(1, "iscntrl('%c') was not false", c2);
	return 0;
}
