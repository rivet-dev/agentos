/* Test if OPEN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef OPEN_MAX
        if ( OPEN_MAX < _POSIX_OPEN_MAX )
                errx(1, "OPEN_MAX < _POSIX_OPEN_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
