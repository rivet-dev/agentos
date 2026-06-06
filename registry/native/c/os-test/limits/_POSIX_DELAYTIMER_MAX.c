/* Test if _POSIX_DELAYTIMER_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_DELAYTIMER_MAX
        if ( _POSIX_DELAYTIMER_MAX != 32 )
                errx(1, "_POSIX_DELAYTIMER_MAX is not 32");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
