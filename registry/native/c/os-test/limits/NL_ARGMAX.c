/* Test if NL_ARGMAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NL_ARGMAX
        if ( NL_ARGMAX < 9 )
                errx(1, "NL_ARGMAX < 9");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
