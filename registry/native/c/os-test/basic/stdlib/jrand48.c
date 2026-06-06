/*[XSI]*/
/* Test whether a basic jrand48 invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	unsigned short xsubi[3] = { 42, 1337, 9001 };
	long value = jrand48(xsubi);
	if ( value < INT32_MIN || INT32_MAX < value )
		err(1, "jrand48 was out of range: %ld", value);
	return 0;
}
