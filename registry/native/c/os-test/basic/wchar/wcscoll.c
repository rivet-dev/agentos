/* Test whether a basic wcscoll invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t a[8] = L"abcdefg";
	wchar_t b[8] = L"abcdeFG";
	int comparison = wcscoll(a, b);
	if ( comparison <= 0 )
		errx(1, "wcscoll gave %d instead of non-negative", comparison);
	return 0;
}
