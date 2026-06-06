/* Test if SCHAR_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SCHAR_MAX
        if ( SCHAR_MAX < 127 )
                errx(1, "SCHAR_MAX < 127");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
