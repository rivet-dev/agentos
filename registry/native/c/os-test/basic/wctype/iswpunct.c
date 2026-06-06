/* Test whether a basic iswpunct invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'.';
	wchar_t wc2 = L'A';
	if ( !iswpunct(wc1) )
		errx(1, "iswpunct('%lc') was not true", wc1);
	if ( iswpunct(wc2) )
		errx(1, "iswpunct('%lc') was not false", wc2);
	return 0;
}
