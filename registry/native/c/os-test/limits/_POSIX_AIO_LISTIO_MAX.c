/* Test if _POSIX_AIO_LISTIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_AIO_LISTIO_MAX
        if ( _POSIX_AIO_LISTIO_MAX != 2 )
                errx(1, "_POSIX_AIO_LISTIO_MAX is not 2");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
