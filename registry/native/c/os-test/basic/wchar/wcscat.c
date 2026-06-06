/* Test whether a basic wcscat invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t dst[8] = L"foo";
	if ( wcscat(dst, L"bar") != dst )
		errx(1, "wcscat did not return pointer to dst's end");
	const wchar_t* expected = L"foobar";
	if ( wcscmp(dst, expected) != 0 )
		errx(1, "wcscat gave %ls not %ls", dst, expected);
	return 0;
}
