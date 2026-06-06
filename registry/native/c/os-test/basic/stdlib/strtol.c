/* Test whether a basic strtol invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	long value = strtol("-42.1end", &end, 10);
	long expected = -42L;
	if ( value != expected )
		errx(1, "strtol returned %ld rather than %ld", value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtol set wrong end pointer");
	return 0;
}
