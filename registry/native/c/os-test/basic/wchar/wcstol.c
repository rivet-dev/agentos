/* Test whether a basic wcstol invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	long value = wcstol(L"-42.1end", &end, 10);
	long expected = -42L;
	if ( value != expected )
		errx(1, "wcstol returned %ld rather than %ld", value, expected);
	if ( wcscmp(end, L".1end") != 0 )
		errx(1, "wcstol set wrong end pointer");
	return 0;
}
