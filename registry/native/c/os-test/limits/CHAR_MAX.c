/* Test if CHAR_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#if defined(CHAR_MAX) && defined(UCHAR_MAX) && defined(SCHAR_MAX)
        if ( CHAR_MAX != UCHAR_MAX && CHAR_MAX != SCHAR_MAX )
                errx(1, "CHAR_MAX != UCHAR_MAX && CHAR_MAX != SCHAR_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
