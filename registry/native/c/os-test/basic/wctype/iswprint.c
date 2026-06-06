/* Test whether a basic iswprint invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'A';
	wchar_t wc2 = L'\r';
	if ( !iswprint(wc1) )
		errx(1, "iswprint('%lc') was not true", wc1);
	if ( iswprint(wc2) )
		errx(1, "iswprint('%lc') was not false", wc2);
	return 0;
}
