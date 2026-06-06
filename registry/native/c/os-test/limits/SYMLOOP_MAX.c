/* Test if SYMLOOP_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SYMLOOP_MAX
        if ( SYMLOOP_MAX < _POSIX_SYMLOOP_MAX )
                errx(1, "SYMLOOP_MAX < _POSIX_SYMLOOP_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
