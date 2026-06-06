/* Test whether a basic wcstold invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	long double value = wcstold(L"42.1end", &end);
	long double expected = 42.1L;
	if ( value != expected )
		errx(1, "wcstold returned %lf rather than %lf", value, expected);
	if ( wcscmp(end, L"end") != 0 )
		errx(1, "wcstold set wrong end pointer");
	return 0;
}
