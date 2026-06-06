/* Test if SCHAR_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SCHAR_MIN
        if ( SCHAR_MIN > -128 )
                errx(1, "SCHAR_MIN > - 28");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
