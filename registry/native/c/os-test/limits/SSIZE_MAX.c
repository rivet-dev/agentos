/* Test if SSIZE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SSIZE_MAX
        if ( SSIZE_MAX < _POSIX_SSIZE_MAX )
                errx(1, "SSIZE_MAX < _POSIX_SSIZE_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
