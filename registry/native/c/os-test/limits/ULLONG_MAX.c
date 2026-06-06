/* Test if ULLONG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef ULLONG_MAX
        if ( ULLONG_MAX < 18446744073709551615 )
                errx(1, "ULLONG_MAX < 18446744073709551615");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
