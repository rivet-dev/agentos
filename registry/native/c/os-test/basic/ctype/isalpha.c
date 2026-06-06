/* Test whether a basic isalpha invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'A';
	char c2 = '1';
	if ( !isalpha(c1) )
		errx(1, "isalpha('%c') was not true", c1);
	if ( isalpha(c2) )
		errx(1, "isalpha('%c') was not false", c2);
	return 0;
}
