/* Test if _POSIX_CHILD_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_CHILD_MAX
        if ( _POSIX_CHILD_MAX != 25 )
                errx(1, "_POSIX_CHILD_MAX is not 25");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
