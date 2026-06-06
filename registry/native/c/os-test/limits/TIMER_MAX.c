/* Test if TIMER_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef TIMER_MAX
        if ( TIMER_MAX < _POSIX_TIMER_MAX )
                errx(1, "TIMER_MAX < _POSIX_TIMER_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
