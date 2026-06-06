/* Test if _POSIX_ARG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_ARG_MAX
        if ( _POSIX_ARG_MAX != 4096 )
                errx(1, "_POSIX_ARG_MAX is not 4096");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
