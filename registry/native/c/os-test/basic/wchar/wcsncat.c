/* Test whether a basic wcsncat invocation works. */

#include <wchar.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"AB";
	if ( wcsncat(dst, src, 4) != dst )
		errx(1, "wcsncat did not return dst");
	const wchar_t* expected = L"ABabcd";
	if ( wcscmp(dst, expected) != 0 )
		errx(1, "wcsncat gave %ls instead of %ls", dst, expected);
	return 0;
}
