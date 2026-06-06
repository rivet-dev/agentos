/* Test if NL_LANGMAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NL_LANGMAX
        if ( NL_LANGMAX < 14 )
                errx(1, "NL_LANGMAX < 14");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
