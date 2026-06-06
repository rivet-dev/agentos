/* Test whether a basic wmemcmp invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t a[8] = L"abcd\0fg";
	wchar_t b[8] = L"abcd\0FG";
	int comparison = wmemcmp(a, b, 8);
	if ( comparison <= 0 )
		errx(1, "wmemcmp gave %d instead of non-negative", comparison);
	return 0;
}
