/* Test if _POSIX_OPEN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_OPEN_MAX
        if ( _POSIX_OPEN_MAX != 20 )
                errx(1, "_POSIX_OPEN_MAX is not 20");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
