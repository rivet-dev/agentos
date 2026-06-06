/* Test if _POSIX_MAX_INPUT has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_MAX_INPUT
        if ( _POSIX_MAX_INPUT != 255 )
                errx(1, "_POSIX_MAX_INPUT is not 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
