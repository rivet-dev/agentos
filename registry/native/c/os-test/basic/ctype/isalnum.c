/* Test whether a basic isalnum invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'a';
	char c2 = '@';
	if ( !isalnum(c1) )
		errx(1, "isalnum('%c') was not true", c1);
	if ( isalnum(c2) )
		errx(1, "isalnum('%c') was not false", c2);
	return 0;
}
