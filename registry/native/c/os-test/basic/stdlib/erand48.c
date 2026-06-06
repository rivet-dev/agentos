/*[XSI]*/
/* Test whether a basic erand48 invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	unsigned short xsubi[3] = { 42, 1337, 9001 };
	double value = erand48(xsubi);
	if ( value < 0.0 || 1.0 < value )
		err(1, "erand48 was out of range: %f", value);
	return 0;
}
