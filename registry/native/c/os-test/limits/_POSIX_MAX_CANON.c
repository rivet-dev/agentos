/* Test if _POSIX_MAX_CANON has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_MAX_CANON
        if ( _POSIX_MAX_CANON != 255 )
                errx(1, "_POSIX_MAX_CANON is not 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
