/*[XSI]*/
/* Test whether a basic mrand48 invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long value = mrand48();
	if ( value < INT32_MIN || INT32_MAX < value )
		err(1, "mrand48 was out of range: %ld", value);
	return 0;
}
