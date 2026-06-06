/* Test whether a basic free invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* buf = malloc(1);
	if ( !buf )
		err(1, "malloc");
	free(buf);
	free(NULL);
	return 0;
}
