/* Test whether a basic funlockfile invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	flockfile(stdout);
	funlockfile(stdout);
	return 0;
}
