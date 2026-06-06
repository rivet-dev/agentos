/* Test whether a basic iswalnum invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'a';
	wchar_t wc2 = L'@';
	if ( !iswalnum(wc1) )
		errx(1, "iswalnum('%lc') was not true", wc1);
	if ( iswalnum(wc2) )
		errx(1, "iswalnum('%lc') was not false", wc2);
	return 0;
}
