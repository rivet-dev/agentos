/* Test whether a basic fegetexceptflag invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	fexcept_t flags;
	if ( fegetexceptflag(&flags, FE_ALL_EXCEPT) )
		errx(1, "fegetexceptflag");
	return 0;
}
