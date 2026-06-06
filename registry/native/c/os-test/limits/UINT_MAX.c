/* Test if UINT_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef UINT_MAX
        if ( UINT_MAX < 4294967295 )
                errx(1, "UINT_MAX < 4294967295");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
