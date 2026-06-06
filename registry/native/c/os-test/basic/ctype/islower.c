/* Test whether a basic islower invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'a';
	char c2 = 'A';
	if ( !islower(c1) )
		errx(1, "islower('%c') was not true", c1);
	if ( islower(c2) )
		errx(1, "islower('%c') was not false", c2);
	return 0;
}
