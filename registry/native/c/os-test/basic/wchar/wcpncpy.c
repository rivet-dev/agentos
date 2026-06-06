/* Test whether a basic wcpncpy invocation works. */

#include <wchar.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wstringop-truncation"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"ABCDEFG";
	if ( wcpncpy(dst, src, 4) != dst + 4 )
		errx(1, "wcpncpy did not return pointer to dst's end");
	wchar_t expected[8] = L"abcdEFG";
	if ( wmemcmp(dst, expected, 8) != 0 )
		errx(1, "wcpncpy did not copy properly");
	return 0;
}
