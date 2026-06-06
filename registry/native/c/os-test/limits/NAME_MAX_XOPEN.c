/*[XSI]*/
/* Test if NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NAME_MAX
        if ( NAME_MAX < _XOPEN_NAME_MAX )
                errx(1, "NAME_MAX < _XOPEN_NAME_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
