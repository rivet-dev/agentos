/* Test whether a basic iswalpha invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'A';
	wchar_t wc2 = L'1';
	if ( !iswalpha(wc1) )
		errx(1, "iswalpha('%lc') was not true", wc1);
	if ( iswalpha(wc2) )
		errx(1, "iswalpha('%lc') was not false", wc2);
	return 0;
}
