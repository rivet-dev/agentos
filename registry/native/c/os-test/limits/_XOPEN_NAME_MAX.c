/*[XSI]*/
/* Test if _XOPEN_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _XOPEN_NAME_MAX
        if ( _XOPEN_NAME_MAX != 255 )
                errx(1, "_XOPEN_NAME_MAX is not 255");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
