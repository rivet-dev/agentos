/* Test whether a basic iswcntrl invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'\r';
	wchar_t wc2 = L'x';
	if ( !iswcntrl(wc1) )
		errx(1, "iswcntrl('%lc') was not true", wc1);
	if ( iswcntrl(wc2) )
		errx(1, "iswcntrl('%lc') was not false", wc2);
	return 0;
}
