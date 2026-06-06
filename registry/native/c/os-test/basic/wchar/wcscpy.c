/* Test whether a basic wcscpy invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"ABCDEFG";
	wchar_t* result = wcscpy(dst, src);
	if ( result != dst )
		errx(1, "wcscpy did not return dst");
	const wchar_t* expected = L"abcdefg";
	if ( wcscmp(dst, expected) != 0 )
		errx(1, "wcscpy gave %ls instead of %ls", dst, expected);
	return 0;
}
