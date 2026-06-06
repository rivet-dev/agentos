/*[XSI]*/
/* Test whether a basic lrand48 invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long value = lrand48();
	if ( value < 0 || INT32_MAX < value )
		err(1, "lrand48 was out of range: %ld", value);
	return 0;
}
