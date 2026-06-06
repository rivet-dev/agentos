/* Test if NL_TEXTMAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NL_TEXTMAX
        if ( NL_TEXTMAX < _POSIX2_LINE_MAX )
                errx(1, "NL_TEXTMAX < _POSIX2_LINE_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
