/* Test whether a basic wcslcpy invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"ABCDEFG";
	size_t result = wcslcpy(dst, src, 4);
	if ( result != 7 )
		errx(1, "wcslcpy did not return attempted length");
	const wchar_t* expected = L"abc";
	if ( wcscmp(dst, expected) != 0 )
		errx(1, "wcslcpy gave %ls instead of %ls", dst, expected);
	return 0;
}
