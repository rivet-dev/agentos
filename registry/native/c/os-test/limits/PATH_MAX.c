/* Test if PATH_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PATH_MAX
        if ( PATH_MAX < _POSIX_PATH_MAX )
                errx(1, "PATH_MAX < _POSIX_PATH_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
