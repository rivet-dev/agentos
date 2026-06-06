/* Test whether a basic atoi invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int value = atoi("42");
	if ( value != 42 )
		errx(1, "atoi() was %d, not 42", value);
	return 0;
}
