/*[XSI]*/
/* Test whether a basic drand48 invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	double value = drand48();
	if ( value < 0.0 || 1.0 < value )
		err(1, "drand48 was out of range: %f", value);
	return 0;
}
