/*[XSI]*/
/* Test whether a basic random invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long value = random();
	if ( value < 0 || INT32_MAX < value )
		err(1, "random was out of range: %d", value);
	return 0;
}
