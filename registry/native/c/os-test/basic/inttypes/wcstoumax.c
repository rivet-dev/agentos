/* Test whether a basic wcstoumax invocation works. */

#include <inttypes.h>
#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t* end;
	uintmax_t value = wcstoumax(L"-4611686014132420609.1end", &end, 10);
	uintmax_t expected = (uintmax_t) INTMAX_C(-4611686014132420609);
	if ( value != expected )
		errx(1, "wcstoumax returned %"PRIuMAX" rather than %"PRIuMAX"",
		     value, expected);
	if ( wcscmp(end, L".1end") != 0 )
		errx(1, "wcstoumax set wrong end pointer");
	return 0;
}
