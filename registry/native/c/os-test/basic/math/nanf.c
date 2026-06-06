/* Test whether a basic nanf invocation works. */

#include <errno.h>
#include <fenv.h>
#include <math.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	float d = nanf("");
	if ( !isnan(d) )
		errx(1, "nanf did not return NaN");
	return 0;
}
