/* Test if ULONG_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef ULONG_MAX
        if ( ULONG_MAX < 4294967295 )
                errx(1, "ULONG_MAX < 4294967295");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
