/* Test whether a basic towupper invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'x';
	wchar_t wc2 = L'X';
	wchar_t wc3 = towupper(wc1);
	if ( wc3 != wc2 )
		errx(1, "towupper('%lc') was not '%lc'", wc1, wc2);
	return 0;
}
