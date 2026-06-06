/*[XSI]*/
/* Test whether a basic seed48 invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	unsigned short xsubi[3] = { 42, 1337, 9001 };
	unsigned short* old_state = seed48(xsubi);
	if ( !old_state )
		errx(1, "seed48 returned NULL");
	return 0;
}
