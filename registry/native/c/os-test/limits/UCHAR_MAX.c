/* Test if UCHAR_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef UCHAR_MAX
        if ( UCHAR_MAX < 255 )
                errx(1, "UCHAR_MAX < 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
