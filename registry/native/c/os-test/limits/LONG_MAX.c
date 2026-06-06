/* Test if LONG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LONG_MAX
        if ( LONG_MAX < 2147483647 )
                errx(1, "LONG_MAX < 2147483647");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
