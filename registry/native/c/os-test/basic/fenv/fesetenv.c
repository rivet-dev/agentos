/* Test whether a basic fesetenv invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	fenv_t env;
	if ( fegetenv(&env) )
		errx(1, "fegetenv");
	if ( fesetenv(&env) )
		errx(1, "fesetenv");
	return 0;
}
