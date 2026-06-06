/* Test whether a basic atol invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long value = atol("2147483647");
	if ( value != 2147483647L )
		errx(1, "atol() was %ld, not 2147483647", value);
	return 0;
}
