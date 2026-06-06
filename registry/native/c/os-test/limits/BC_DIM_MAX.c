/* Test if BC_DIM_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef BC_DIM_MAX
        if ( BC_DIM_MAX < _POSIX2_BC_DIM_MAX )
                errx(1, "BC_DIM_MAX < _POSIX2_BC_DIM_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
