/* Test whether a basic mbsnrtowcs invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	const char* str = "foo";
	const char* ptr = str;
	wchar_t wcs[4];
	size_t amount = mbsnrtowcs(wcs, &ptr, 2, 4, &ps);
	if ( amount != 2 )
		err(1, "mbsnrtowcs() != 2");
	if ( wcsncmp(wcs, L"fo", 2) != 0 )
		errx(1, "did not decode \"fo\"");
	if ( ptr != str + 2 )
		errx(1, "wrong output pointer");
	return 0;
}
