/* Test if GETENTROPY_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef GETENTROPY_MAX
        if ( GETENTROPY_MAX < 256 )
                errx(1, "GETENTROPY_MAX < 256");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
