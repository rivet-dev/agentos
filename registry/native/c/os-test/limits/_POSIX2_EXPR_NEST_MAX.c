/* Test if _POSIX2_EXPR_NEST_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_EXPR_NEST_MAX
        if ( _POSIX2_EXPR_NEST_MAX != 32 )
                errx(1, "_POSIX2_EXPR_NEST_MAX is not 32");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
