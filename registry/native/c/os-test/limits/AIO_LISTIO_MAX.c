/* Test if AIO_LISTIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef AIO_LISTIO_MAX
        if ( AIO_LISTIO_MAX < _POSIX_AIO_LISTIO_MAX )
                errx(1, "AIO_LISTIO_MAX < _POSIX_AIO_LISTIO_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
