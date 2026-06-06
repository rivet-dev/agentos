/*[XSI]*/
/* Test if _XOPEN_IOV_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _XOPEN_IOV_MAX
        if ( _XOPEN_IOV_MAX != 16 )
                errx(1, "_XOPEN_IOV_MAX is not 16");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
