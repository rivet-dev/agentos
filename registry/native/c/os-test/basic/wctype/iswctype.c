/* Test whether a basic iswctype invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wctype_t charclass = wctype("alpha");
	if ( !charclass )
		errx(1, "wctype failed");
	wchar_t wc1 = L'a';
	wchar_t wc2 = L'1';
	if ( !iswctype(wc1, charclass) )
		errx(1, "iswctype('%lc', charclass) was not true", wc1);
	if ( iswctype(wc2, charclass) )
		errx(1, "iswctype('%lc', charclass) was not false", wc2);
	// "If charclass is (wctype_t)0, the iswctype() and iswctype() functions
	//  shall return 0."
	if ( iswctype(wc1, (wctype_t) 0) )
		errx(1, "iswctype('%lc', (wctype_t) 0) was not false", wc1);
	if ( iswctype(wc2, (wctype_t) 0) )
		errx(1, "iswctype('%lc', (wctype_t) 0) was not false", wc2);
	return 0;
}
