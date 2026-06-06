/* Test whether a basic strtold invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	long double value = strtold("42.1end", &end);
	long double expected = 42.1L;
	if ( value != expected )
		errx(1, "strtold returned %lf rather than %lf", value, expected);
	if ( strcmp(end, "end") != 0 )
		errx(1, "strtold set wrong end pointer");
	return 0;
}
