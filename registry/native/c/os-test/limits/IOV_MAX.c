/*[XSI]*/
/* Test if IOV_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef IOV_MAX
        if ( IOV_MAX < _XOPEN_IOV_MAX )
                errx(1, "IOV_MAX < _XOPEN_IOV_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
