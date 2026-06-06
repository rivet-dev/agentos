/* Test if RTSIG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef RTSIG_MAX
        if ( RTSIG_MAX < _POSIX_RTSIG_MAX )
                errx(1, "RTSIG_MAX < _POSIX_RTSIG_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
