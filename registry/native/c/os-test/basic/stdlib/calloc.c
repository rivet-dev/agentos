/* Test whether a basic calloc invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int* integers = calloc(4, sizeof(int));
	if ( !integers )
		err(1, "calloc");
	for ( size_t i = 0; i < 4; i++ )
	{
		if ( integers[i] != 0 )
			err(1, "calloc did not zero initialize");
		integers[i] = i;
		if ( i )
			integers[i] += integers[i - 1];
	}
	free(integers);
	return 0;
}
