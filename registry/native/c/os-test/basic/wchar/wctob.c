/* Test whether a basic wctob invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wctob(L'A') != 'A' )
		errx(1, "wctob(L'A') != 'A'");
	return 0;
}
