/* Test whether a basic wcstoull invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	unsigned long long value = wcstoull(L"-4611686014132420609.1end", &end, 10);
	unsigned long long expected = (unsigned long long) -4611686014132420609LL;
	if ( value != expected )
		errx(1, "wcstoull returned %lld rather than %lld", value, expected);
	if ( wcscmp(end, L".1end") != 0 )
		errx(1, "wcstoull set wrong end pointer");
	return 0;
}
