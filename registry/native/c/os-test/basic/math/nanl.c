/* Test whether a basic nanl invocation works. */

#include <errno.h>
#include <fenv.h>
#include <math.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	long double d = nanl("");
	if ( !isnan(d) )
		errx(1, "nanl did not return NaN");
	return 0;
}
