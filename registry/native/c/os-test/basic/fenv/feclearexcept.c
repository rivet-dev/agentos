/* Test whether a basic feclearexcept invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

#if defined(FE_DIVBYZERO)
#define EXCEPTION FE_DIVBYZERO
#elif defined(FE_INEXACT)
#define EXCEPTION FE_INEXACT
#elif defined(FE_INVALID)
#define EXCEPTION FE_INVALID
#elif defined(FE_OVERFLOW)
#define EXCEPTION FE_OVERFLOW
#elif defined(FE_UNDERFLOW)
#define EXCEPTION FE_UNDERFLOW
#endif

int main(void)
{
	#pragma STDC FENV_ACCESS ON
	if ( fetestexcept(FE_ALL_EXCEPT) != 0 )
		errx(1, "first fetestexcept() != 0");
#ifdef EXCEPTION
	if ( feraiseexcept(EXCEPTION) )
		errx(1, "feraiseexcept failed");
	if ( fetestexcept(EXCEPTION) == 0 )
		errx(1, "second fetestexcept() == 0");
#endif
	if ( feclearexcept(EXCEPTION) )
		errx(1, "feclearexcept");
	if ( fetestexcept(EXCEPTION) != 0 )
		errx(1, "third fetestexcept() != 0");
	return 0;
}
