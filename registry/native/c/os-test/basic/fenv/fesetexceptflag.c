/* Test whether a basic fesetexceptflag invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	fexcept_t flags;
	if ( fegetexceptflag(&flags, FE_ALL_EXCEPT) )
		errx(1, "fegetexceptflag");
	if ( fesetexceptflag(&flags, FE_ALL_EXCEPT) )
		errx(1, "fesetexceptflag");
	return 0;
}
