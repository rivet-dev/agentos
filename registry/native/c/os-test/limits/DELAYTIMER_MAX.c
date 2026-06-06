/* Test if DELAYTIMER_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef DELAYTIMER_MAX
        if ( DELAYTIMER_MAX < _POSIX_DELAYTIMER_MAX )
                errx(1, "DELAYTIMER_MAX < _POSIX_DELAYTIMER_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
