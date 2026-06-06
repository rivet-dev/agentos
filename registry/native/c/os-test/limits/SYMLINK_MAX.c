/* Test if SYMLINK_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SYMLINK_MAX
        if ( SYMLINK_MAX < _POSIX_SYMLINK_MAX )
                errx(1, "SYMLINK_MAX < _POSIX_SYMLINK_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
