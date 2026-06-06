/* Test whether a basic isgraph invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'x';
	char c2 = ' ';
	if ( !isgraph(c1) )
		errx(1, "isgraph('%c') was not true", c1);
	if ( isgraph(c2) )
		errx(1, "isgraph('%c') was not false", c2);
	return 0;
}
