/* Test whether a basic iswblank invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L' ';
	wchar_t wc2 = L'_';
	if ( !iswblank(wc1) )
		errx(1, "iswblank('%lc') was not true", wc1);
	if ( iswblank(wc2) )
		errx(1, "iswblank('%lc') was not false", wc2);
	return 0;
}
