/* Test if FILESIZEBITS has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef FILESIZEBITS
        if ( FILESIZEBITS < 32 )
                errx(1, "FILESIZEBITS < 32");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
