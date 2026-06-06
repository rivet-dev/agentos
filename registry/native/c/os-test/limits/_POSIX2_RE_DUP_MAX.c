/* Test if _POSIX2_RE_DUP_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX2_RE_DUP_MAX
        if ( _POSIX2_RE_DUP_MAX != 255 )
                errx(1, "_POSIX2_RE_DUP_MAX is not 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
