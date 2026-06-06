/* Test if NL_SETMAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NL_SETMAX
        if ( NL_SETMAX < 255 )
                errx(1, "NL_SETMAX < 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
