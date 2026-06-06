/* Test if _POSIX_TZNAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_TZNAME_MAX
        if ( _POSIX_TZNAME_MAX != 6 )
                errx(1, "_POSIX_TZNAME_MAX is not 6");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
