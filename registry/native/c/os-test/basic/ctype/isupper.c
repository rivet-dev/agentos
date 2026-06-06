/* Test whether a basic isupper invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'A';
	char c2 = 'a';
	if ( !isupper(c1) )
		errx(1, "isupper('%c') was not true", c1);
	if ( isupper(c2) )
		errx(1, "isupper('%c') was not false", c2);
	return 0;
}
