/* Test whether a basic towctrans invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wctrans_t desc = wctrans("tolower");
	if ( !desc )
		err(1, "wctrans");
	wchar_t wc1 = L'X';
	wchar_t wc2 = L'x';
	wchar_t wc3 = towctrans(wc1, desc);
	if ( wc3 != wc2 )
		errx(1, "wctrans('%lc', desc) was not '%lc'", wc1, wc2);
	return 0;
}
