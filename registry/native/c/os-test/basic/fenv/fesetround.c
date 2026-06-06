/* Test whether a basic fesetround invocation works. */

#include <fenv.h>

#include "../basic.h"

#pragma STDC FENV_ACCESS ON

#if defined(FE_DOWNWARD)
#define ROUNDING FE_DOWNWARD
#elif defined(FE_TONEAREST)
#define ROUNDING FE_TONEAREST
#elif defined(FE_TOWARDZERO)
#define ROUNDING FE_TOWARDZERO
#elif defined(FE_UPWARD)
#define ROUNDING FE_UPWARD
#endif

int main(void)
{
	if ( fegetround() < 0 )
		errx(1, "first fegetround failed");
#ifdef ROUNDING
	if ( fesetround(ROUNDING) < 0 )
		errx(1, "fsgetround failed");
	if ( fegetround() != ROUNDING )
		errx(1, "second fegetround() != ROUNDING");
#endif
	return 0;
}
