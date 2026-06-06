/* Test whether a basic fegetround invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

int main(void)
{
	if ( fegetround() < 0 )
		errx(1, "first fegetround failed");
	return 0;
}
