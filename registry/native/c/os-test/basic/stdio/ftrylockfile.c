/* Test whether a basic ftrylockfile invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	if ( ftrylockfile(stdout) )
		errx(1, "ftrylockfile failed");
	return 0;
}
