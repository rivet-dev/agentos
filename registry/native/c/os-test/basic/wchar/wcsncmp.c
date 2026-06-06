/* Test whether a basic wcsncmp invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t a[8] = L"abcdefg";
	wchar_t b[8] = L"abcdeFG";
	int comparison = wcsncmp(a, b, 5);
	if ( comparison != 0 )
		errx(1, "wcsncmp gave %d instead of 0", comparison);
	return 0;
}
