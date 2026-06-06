/* Test whether a basic mbsrtowcs invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	const char* str = "foo";
	const char* ptr = str;
	wchar_t wcs[4];
	size_t amount = mbsrtowcs(wcs, &ptr, 4, &ps);
	if ( amount != 3 )
		err(1, "mbsrtowcs() != 3");
	if ( wcscmp(wcs, L"foo") != 0 )
		errx(1, "did not decode \"foo\"");
	if ( ptr )
		errx(1, "wrong output pointer");
	return 0;
}
