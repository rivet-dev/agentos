/* Test if HOST_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef HOST_NAME_MAX
        if ( HOST_NAME_MAX < _POSIX_HOST_NAME_MAX )
                errx(1, "HOST_NAME_MAX < _POSIX_HOST_NAME_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
