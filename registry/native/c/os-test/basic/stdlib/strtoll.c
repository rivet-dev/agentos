/* Test whether a basic strtoll invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	long long value = strtoll("-4611686014132420609.1end", &end, 10);
	long long expected = -4611686014132420609LL;
	if ( value != expected )
		errx(1, "strtoll returned %lld rather than %lld", value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtoll set wrong end pointer");
	return 0;
}
