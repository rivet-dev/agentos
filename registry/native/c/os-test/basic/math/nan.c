/* Test whether a basic nan invocation works. */

#include <errno.h>
#include <fenv.h>
#include <math.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	double d = nan("");
	if ( !isnan(d) )
		errx(1, "nan did not return NaN");
	return 0;
}
