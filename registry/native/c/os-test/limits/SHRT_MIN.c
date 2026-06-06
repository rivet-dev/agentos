/* Test if SHRT_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SHRT_MIN
        if ( SHRT_MIN > -32768 )
                errx(1, "SHRT_MIN > -32768");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
