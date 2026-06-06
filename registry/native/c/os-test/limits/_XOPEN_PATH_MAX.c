/*[XSI]*/
/* Test if _XOPEN_PATH_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _XOPEN_PATH_MAX
        if ( _XOPEN_PATH_MAX != 1024 )
                errx(1, "_XOPEN_PATH_MAX is not 1024");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
