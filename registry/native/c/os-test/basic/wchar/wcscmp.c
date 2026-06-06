/* Test whether a basic wcscmp invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t a[8] = L"abcdefg";
	wchar_t b[8] = L"abcdeFG";
	int comparison = wcscmp(a, b);
	if ( comparison <= 0 )
		errx(1, "wcscmp gave %d instead of non-negative", comparison);
	return 0;
}
