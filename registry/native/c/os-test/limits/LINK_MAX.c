/* Test if LINK_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LINK_MAX
        if ( LINK_MAX < _POSIX_LINK_MAX )
                errx(1, "LINK_MAX < _POSIX_LINK_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
