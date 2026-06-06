/* Test whether a basic isdigit invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = '1';
	char c2 = 'a';
	if ( !isdigit(c1) )
		errx(1, "isdigit('%c') was not true", c1);
	if ( isdigit(c2) )
		errx(1, "isdigit('%c') was not false", c2);
	return 0;
}
