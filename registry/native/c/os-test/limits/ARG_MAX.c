/* Test if ARG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef ARG_MAX
        if ( ARG_MAX < _POSIX_ARG_MAX )
                errx(1, "ARG_MAX < _POSIX_ARG_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
