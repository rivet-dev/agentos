/* Test whether a basic atof invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	double value = atof("42.1");
	if ( value != 42.1 )
		errx(1, "atof() was %f, not 42.1", value);
	return 0;
}
