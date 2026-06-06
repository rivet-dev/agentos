/* Test whether a basic towlower invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'X';
	wchar_t wc2 = L'x';
	wchar_t wc3 = towlower(wc1);
	if ( wc3 != wc2 )
		errx(1, "towlower('%lc') was not '%lc'", wc1, wc2);
	return 0;
}
