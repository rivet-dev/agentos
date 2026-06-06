/* Test whether a basic wcstod invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	double value = wcstod(L"42.1end", &end);
	double expected = 42.1;
	if ( value != expected )
		errx(1, "wcstod returned %f rather than %f", value, expected);
	if ( wcscmp(end, L"end") != 0 )
		errx(1, "wcstod set wrong end pointer");
	return 0;
}
