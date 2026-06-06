/* Test whether a basic malloc invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	void* ptr = malloc(42);
	if ( !ptr )
		err(1, "malloc");
	return 0;
}
