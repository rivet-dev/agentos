/* Test if AIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef AIO_MAX
        if ( AIO_MAX < _POSIX_AIO_MAX )
                errx(1, "AIO_MAX < _POSIX_AIO_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
