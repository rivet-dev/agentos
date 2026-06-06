/* Test if COLL_WEIGHTS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef COLL_WEIGHTS_MAX
        if ( COLL_WEIGHTS_MAX < _POSIX2_COLL_WEIGHTS_MAX )
                errx(1, "COLL_WEIGHTS_MAX < _POSIX2_COLL_WEIGHTS_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
