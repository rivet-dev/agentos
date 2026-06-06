/* Test whether a basic strtoumax invocation works. */

#include <inttypes.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char* end;
	uintmax_t value = strtoull("-4611686014132420609.1end", &end, 10);
	uintmax_t expected = (uintmax_t) INTMAX_C(-4611686014132420609);
	if ( value != expected )
		errx(1, "strtoull returned %"PRIuMAX" rather than %"PRIuMAX"",
		     value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtoull set wrong end pointer");
	return 0;
}
