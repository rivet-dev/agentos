/* Test whether a basic atexit invocation works. */

#include <stdlib.h>

#include "../basic.h"

static void cleanup(void)
{
	_Exit(0);
}

int main(void)
{
	if ( atexit(cleanup) < 0 )
		err(1, "atexit");
	return 1;
}
