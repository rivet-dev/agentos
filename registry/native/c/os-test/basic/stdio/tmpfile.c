/* Test whether a basic tmpfile invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	if ( !tmpfile() )
		err(1, "tmpfile");
	return 0;
}
