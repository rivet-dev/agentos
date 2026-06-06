/* Test whether a basic fegetenv invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	fenv_t env;
	if ( fegetenv(&env) )
		errx(1, "fegetenv");
	return 0;
}
