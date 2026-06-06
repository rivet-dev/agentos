/* Test if _POSIX_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_NAME_MAX
        if ( _POSIX_NAME_MAX != 14 )
                errx(1, "_POSIX_NAME_MAX is not 14");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
