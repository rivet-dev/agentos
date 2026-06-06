/* Test if LLONG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LLONG_MAX
        if ( LLONG_MAX < 9223372035854775807 )
                errx(1, "LLONG_MAX < 9223372035854775807");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
