/* Test whether a basic toupper invocation works. */

#include <ctype.h>

#include "../basic.h"

int main(void)
{
	char c1 = 'x';
	char c2 = 'X';
	char c3 = toupper(c1);
	if ( c3 != c2 )
		errx(1, "toupper('%c') was not '%c'", c1, c2);
	return 0;
}
