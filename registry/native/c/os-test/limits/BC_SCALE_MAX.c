/* Test if BC_SCALE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef BC_SCALE_MAX
        if ( BC_SCALE_MAX < _POSIX2_BC_SCALE_MAX )
                errx(1, "BC_SCALE_MAX < _POSIX2_BC_SCALE_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
