/* Test if LONG_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LONG_MIN
        if ( LONG_MIN > -2147483648 )
                errx(1, "LONG_MIN > -2147483648");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
