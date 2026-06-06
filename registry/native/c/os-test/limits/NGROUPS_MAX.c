/* Test if NGROUPS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NGROUPS_MAX
        if ( NGROUPS_MAX < _POSIX_NGROUPS_MAX )
                errx(1, "NGROUPS_MAX < _POSIX_NGROUPS_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
