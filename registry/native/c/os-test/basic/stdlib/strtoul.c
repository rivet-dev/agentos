/* Test whether a basic strtoul invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	unsigned long value = strtoul("-42.1end", &end, 10);
	unsigned long expected = (unsigned long) -42L;
	if ( value != expected )
		errx(1, "strtoul returned %ld rather than %ld", value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtoul set wrong end pointer");
	return 0;
}
