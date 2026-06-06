/*[XSI]*/
/* Test whether a basic nrand48 invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	unsigned short xsubi[3] = { 42, 1337, 9001 };
	long value = nrand48(xsubi);
	if ( value < 0 || INT32_MAX < value )
		err(1, "nrand48 was out of range: %ld", value);
	return 0;
}
