/* Test if LINE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LINE_MAX
        if ( LINE_MAX < _POSIX2_LINE_MAX )
                errx(1, "LINE_MAX < _POSIX2_LINE_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
