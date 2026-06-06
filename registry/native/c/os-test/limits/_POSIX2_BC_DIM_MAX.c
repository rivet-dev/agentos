/* Test if _POSIX2_BC_DIM_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_BC_DIM_MAX
        if ( _POSIX2_BC_DIM_MAX != 2048 )
                errx(1, "_POSIX2_BC_DIM_MAX is not 2048");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
