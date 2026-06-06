/* Test if ATEXIT_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef ATEXIT_MAX
        if ( ATEXIT_MAX < 32 )
                errx(1, "ATEXIT_MAX < 32");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
