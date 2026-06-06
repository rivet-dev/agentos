/* Test whether a basic wmemcpy invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8] = L"ABCDEFG";
	void* result = wmemcpy(dst, src, 3);
	if ( result != dst )
		errx(1, "wmemcpy did not return dst");
	const wchar_t* expected = L"abcDEFG";
	if ( wmemcmp(dst, expected, 8) != 0 )
		errx(1, "wmemcpy gave %ls instead of %ls", dst, expected);
	return 0;
}
