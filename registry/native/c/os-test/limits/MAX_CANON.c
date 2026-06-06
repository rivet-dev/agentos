/* Test if MAX_CANON has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef MAX_CANON
        if ( MAX_CANON < _POSIX_MAX_CANON )
                errx(1, "MAX_CANON < _POSIX_MAX_CANON");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
