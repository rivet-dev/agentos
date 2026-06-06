/* Test whether a basic wcslcat invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"AB";
	size_t result = wcslcat(dst, src, 8);
	if ( result != 9 )
		errx(1, "wcslcat did not return attempted length");
	const wchar_t* expected = L"ABabcde";
	if ( wcscmp(dst, expected) != 0 )
		errx(1, "wcslcat gave %ls instead of %ls", dst, expected);
	return 0;
}
