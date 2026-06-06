/* Test if _POSIX2_LINE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_LINE_MAX
        if ( _POSIX2_LINE_MAX != 2048 )
                errx(1, "_POSIX2_LINE_MAX is not 2048");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
