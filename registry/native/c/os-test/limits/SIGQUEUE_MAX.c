/* Test if SIGQUEUE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SIGQUEUE_MAX
        if ( SIGQUEUE_MAX < _POSIX_SIGQUEUE_MAX )
                errx(1, "SIGQUEUE_MAX < _POSIX_SIGQUEUE_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
