/* Test if _POSIX2_COLL_WEIGHTS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_COLL_WEIGHTS_MAX
        if ( _POSIX2_COLL_WEIGHTS_MAX != 2 )
                errx(1, "_POSIX2_COLL_WEIGHTS_MAX is not 2");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
