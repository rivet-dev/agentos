/* Test whether a basic feupdateenv invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	fenv_t env;
	if ( feholdexcept(&env) )
		errx(1, "feholdexcept");
	if ( feupdateenv(&env) )
		errx(1, "feupdateenv");
	return 0;
}
