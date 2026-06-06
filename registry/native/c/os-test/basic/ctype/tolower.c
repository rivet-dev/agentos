/* Test whether a basic tolower invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'X';
	char c2 = 'x';
	char c3 = tolower(c1);
	if ( c3 != c2 )
		errx(1, "tolower('%c') was not '%c'", c1, c2);
	return 0;
}
