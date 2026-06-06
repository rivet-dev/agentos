/* Test whether a basic rand invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int value = rand();
	if ( value < 0 || RAND_MAX < value )
		err(1, "rand was out of range: %d", value);
	return 0;
}
