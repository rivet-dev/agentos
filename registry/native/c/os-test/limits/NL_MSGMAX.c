/* Test if NL_MSGMAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NL_MSGMAX
        if ( NL_MSGMAX < 32767 )
                errx(1, "NL_MSGMAX < 32767");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
