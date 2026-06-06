/* Test whether a basic mbstowcs invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char str[5] = "abcd";
	wchar_t wcs[5];
	size_t amount = mbstowcs(wcs, str, sizeof(wcs) / sizeof(wcs[0]));
	if ( amount == (size_t) -1 )
		err(1, "mbstowcs");
	size_t expected = 4;
	if ( amount != expected )
		errx(1, "mbstowcs returned %zu, not %zu", amount, expected);
	if ( wcs[0] != L'a' || wcs[1] != L'b' || wcs[2] != L'c' || wcs[3] != L'd' )
		errx(1, "mbstowcs decoded incorrectly");
	return 0;
}
