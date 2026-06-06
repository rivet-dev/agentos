/* Test if CHILD_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef CHILD_MAX
        if ( CHILD_MAX < _POSIX_CHILD_MAX )
                errx(1, "CHILD_MAX < _POSIX_CHILD_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
