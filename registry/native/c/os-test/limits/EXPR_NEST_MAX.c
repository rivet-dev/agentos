/* Test if EXPR_NEST_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef EXPR_NEST_MAX
        if ( EXPR_NEST_MAX < _POSIX2_EXPR_NEST_MAX )
                errx(1, "EXPR_NEST_MAX < _POSIX2_EXPR_NEST_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
