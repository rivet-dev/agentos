/* Test if TZNAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef TZNAME_MAX
        if ( TZNAME_MAX < _POSIX_TZNAME_MAX )
                errx(1, "TZNAME_MAX < _POSIX_TZNAME_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
