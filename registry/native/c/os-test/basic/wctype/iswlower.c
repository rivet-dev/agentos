/* Test whether a basic iswlower invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'a';
	wchar_t wc2 = L'A';
	if ( !iswlower(wc1) )
		errx(1, "iswlower('%lc') was not true", wc1);
	if ( iswlower(wc2) )
		errx(1, "iswlower('%lc') was not false", wc2);
	return 0;
}
