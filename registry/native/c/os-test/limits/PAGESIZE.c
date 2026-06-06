/* Test if PAGESIZE has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PAGESIZE
        if ( PAGESIZE < 1 )
                errx(1, "PAGESIZE < 1");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
