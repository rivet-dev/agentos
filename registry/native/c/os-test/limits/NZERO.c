/* Test if NZERO has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef NZERO
        if ( NZERO < 20 )
                errx(1, "NZERO < 20");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
