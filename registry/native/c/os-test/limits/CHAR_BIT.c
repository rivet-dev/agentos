/* Test if CHAR_BIT has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef CHAR_BIT
        if ( CHAR_BIT != 8 )
                errx(1, "CHAR_BIT != 8");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
