/* Test whether a basic wcstof invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	float value = wcstof(L"42.1end", &end);
	double expected = 42.1;
	double error = 42.1 - value;
	if ( error < -0.00001 || 0.00001 < error )
		errx(1, "wcstof returned %f rather than %f with error %f",
		        value, expected, error);
	if ( wcscmp(end, L"end") != 0 )
		errx(1, "wcstof set wrong end pointer");
	return 0;
}
