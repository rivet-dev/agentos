/* Test if _POSIX_AIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_AIO_MAX
        if ( _POSIX_AIO_MAX != 1 )
                errx(1, "_POSIX_AIO_MAX is not 1");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
