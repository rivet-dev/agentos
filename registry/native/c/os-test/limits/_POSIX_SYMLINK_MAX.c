/* Test if _POSIX_SYMLINK_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_SYMLINK_MAX
        if ( _POSIX_SYMLINK_MAX != 255 )
                errx(1, "_POSIX_SYMLINK_MAX is not 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
