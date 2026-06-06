/* Test whether a basic iswupper invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wchar_t wc1 = L'A';
	wchar_t wc2 = L'a';
	if ( !iswupper(wc1) )
		errx(1, "iswupper('%lc') was not true", wc1);
	if ( iswupper(wc2) )
		errx(1, "iswupper('%lc') was not false", wc2);
	return 0;
}
