/* Test whether a basic ispunct invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = '.';
	char c2 = 'A';
	if ( !ispunct(c1) )
		errx(1, "ispunct('%c') was not true", c1);
	if ( ispunct(c2) )
		errx(1, "ispunct('%c') was not false", c2);
	return 0;
}
