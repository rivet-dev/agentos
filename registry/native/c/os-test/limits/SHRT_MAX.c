/* Test if SHRT_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SHRT_MAX
        if ( SHRT_MAX < 32767 )
                errx(1, "SHRT_MAX < 32767");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
