/* Test if SEM_VALUE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SEM_VALUE_MAX
        if ( SEM_VALUE_MAX < _POSIX_SEM_VALUE_MAX )
                errx(1, "SEM_VALUE_MAX < _POSIX_SEM_VALUE_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
