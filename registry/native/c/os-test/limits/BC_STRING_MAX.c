/* Test if BC_STRING_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef BC_STRING_MAX
        if ( BC_STRING_MAX < _POSIX2_BC_STRING_MAX )
                errx(1, "BC_STRING_MAX < _POSIX2_BC_STRING_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
