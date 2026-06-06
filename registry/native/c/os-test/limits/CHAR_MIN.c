/* Test if CHAR_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#if defined(CHAR_MIN) && defined(SCHAR_MIN)
        if ( CHAR_MIN != 0 && CHAR_MIN != SCHAR_MIN )
                errx(1, "CHAR_MIN != 0 && CHAR_MIN != SCHAR_MIN");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
