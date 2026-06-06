/* Test whether a basic iswxdigit invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'f';
	wchar_t wc2 = L'g';
	if ( !iswxdigit(wc1) )
		errx(1, "iswxdigit('%lc') was not true", wc1);
	if ( iswxdigit(wc2) )
		errx(1, "iswxdigit('%lc') was not false", wc2);
	return 0;
}
