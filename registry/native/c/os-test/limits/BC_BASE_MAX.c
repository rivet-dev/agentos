/* Test if BC_BASE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef BC_BASE_MAX
        if ( BC_BASE_MAX < _POSIX2_BC_BASE_MAX )
                errx(1, "BC_BASE_MAX < _POSIX2_BC_BASE_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
