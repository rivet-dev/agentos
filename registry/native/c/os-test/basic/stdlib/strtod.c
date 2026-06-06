/* Test whether a basic strtod invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	double value = strtod("42.1end", &end);
	double expected = 42.1;
	if ( value != expected )
		errx(1, "strtod returned %f rather than %f", value, expected);
	if ( strcmp(end, "end") != 0 )
		errx(1, "strtod set wrong end pointer");
	return 0;
}
