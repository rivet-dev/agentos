/* Test if CHARCLASS_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef CHARCLASS_NAME_MAX
        if ( CHARCLASS_NAME_MAX < _POSIX2_CHARCLASS_NAME_MAX )
                errx(1, "CHARCLASS_NAME_MAX < _POSIX2_CHARCLASS_NAME_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
