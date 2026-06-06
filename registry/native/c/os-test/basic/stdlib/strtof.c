/* Test whether a basic strtof invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* end;
	float value = strtof("42.1end", &end);
	double expected = 42.1;
	double error = 42.1 - value;
	if ( error < -0.00001 || 0.00001 < error )
		errx(1, "strtof returned %f rather than %f with error %f",
		        value, expected, error);
	if ( strcmp(end, "end") != 0 )
		errx(1, "strtof set wrong end pointer");
	return 0;
}
