/* Test if _POSIX2_BC_SCALE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_BC_SCALE_MAX
        if ( _POSIX2_BC_SCALE_MAX != 99 )
                errx(1, "_POSIX2_BC_SCALE_MAX is not 99");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
