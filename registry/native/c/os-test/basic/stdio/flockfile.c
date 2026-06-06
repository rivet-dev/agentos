/* Test whether a basic flockfile invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	flockfile(stdout);
	return 0;
}
