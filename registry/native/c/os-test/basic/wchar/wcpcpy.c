/* Test whether a basic wcpcpy invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t src[8] = L"abcdefg";
	wchar_t dst[8];
	if ( wcpcpy(dst, src) != dst + 7 )
		errx(1, "wcpcpy did not return pointer to dst's end");
	if ( wcscmp(src, dst) != 0 )
		errx(1, "wcpcpy did not copy the string");
	return 0;
}
