/* Test whether a basic wcstoul invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	unsigned long value = wcstoul(L"-42.1end", &end, 10);
	unsigned long expected = (unsigned long) -42L;
	if ( value != expected )
		errx(1, "wcstoul returned %ld rather than %ld", value, expected);
	if ( wcscmp(end, L".1end") != 0 )
		errx(1, "wcstoul set wrong end pointer");
	return 0;
}
