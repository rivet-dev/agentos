/* Test if USHRT_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef USHRT_MAX
        if ( USHRT_MAX < 65535 )
                errx(1, "USHRT_MAX < 65535");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
