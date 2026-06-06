/* Test whether a basic iswdigit invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'1';
	wchar_t wc2 = L'a';
	if ( !iswdigit(wc1) )
		errx(1, "iswdigit('%lc') was not true", wc1);
	if ( iswdigit(wc2) )
		errx(1, "iswdigit('%lc') was not false", wc2);
	return 0;
}
