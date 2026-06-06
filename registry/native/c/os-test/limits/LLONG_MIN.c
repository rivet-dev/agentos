/* Test if LLONG_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LLONG_MIN
        if ( LLONG_MIN > -9223372035854775808 )
                errx(1, "LLONG_MIN > -9223372035854775808");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
