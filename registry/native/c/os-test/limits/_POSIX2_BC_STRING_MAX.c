/* Test if _POSIX2_BC_STRING_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_BC_STRING_MAX
        if ( _POSIX2_BC_STRING_MAX != 1000 )
                errx(1, "_POSIX2_BC_STRING_MAX is not 1000");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
