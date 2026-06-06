/* Test whether a basic iswspace invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L' ';
	wchar_t wc2 = L'A';
	if ( !iswspace(wc1) )
		errx(1, "iswspace('%lc') was not true", wc1);
	if ( iswspace(wc2) )
		errx(1, "iswspace('%lc') was not false", wc2);
	return 0;
}
