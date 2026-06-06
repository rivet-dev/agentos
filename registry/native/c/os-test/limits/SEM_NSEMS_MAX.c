/* Test if SEM_NSEMS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SEM_NSEMS_MAX
        if ( SEM_NSEMS_MAX < _POSIX_SEM_NSEMS_MAX )
                errx(1, "SEM_NSEMS_MAX < _POSIX_SEM_NSEMS_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
