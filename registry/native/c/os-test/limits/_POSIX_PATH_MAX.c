/* Test if _POSIX_PATH_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_PATH_MAX
        if ( _POSIX_PATH_MAX != 256 )
                errx(1, "_POSIX_PATH_MAX is not 256");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
