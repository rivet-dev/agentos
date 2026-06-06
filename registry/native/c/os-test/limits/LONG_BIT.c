/* Test if LONG_BIT has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LONG_BIT
        if ( LONG_BIT < 32)
                errx(1, "LONG_BIT < 32");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
