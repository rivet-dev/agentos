/* Test whether a basic wcsncpy invocation works. */

#include <wchar.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"ABCDEFG";
	if ( wcsncpy(dst, src, 4) != dst )
		errx(1, "wcsncpy did not return dst");
	wchar_t expected[8] = L"abcdEFG";
	if ( wmemcmp(dst, expected, 8) != 0 )
		errx(1, "wcsncpy did not copy properly");
	return 0;
}
