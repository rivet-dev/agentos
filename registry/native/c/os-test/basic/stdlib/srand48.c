/*[XSI]*/
/* Test whether a basic srand48 invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	srand48(0x12345678);
	unsigned short new_seed[] = {1, 2, 3};
	unsigned short* old_state = seed48(new_seed);
	if ( !old_state )
		errx(1, "seed48 returned NULL");
	unsigned short expected[3] = { 0x330E, 0x5678, 0x1234 };
	if ( old_state[0] != expected[0] ||
	     old_state[1] != expected[1] ||
	     old_state[2] != expected[2] )
		errx(1, "got state (%x, %x, %x) expected (%x, %x, %x)",
		     old_state[0], old_state[1], old_state[2],
		     expected[0], expected[1], expected[2]);
	return 0;
}
