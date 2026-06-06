/* Test whether a basic strtoull invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	unsigned long long value = strtoull("-4611686014132420609.1end", &end, 10);
	unsigned long long expected = (unsigned long long) -4611686014132420609LL;
	if ( value != expected )
		errx(1, "strtoull returned %lld rather than %lld", value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtoull set wrong end pointer");
	return 0;
}
