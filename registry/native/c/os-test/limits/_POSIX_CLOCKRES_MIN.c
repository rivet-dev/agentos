/* Test if _POSIX_CLOCKRES_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_CLOCKRES_MIN
        if ( _POSIX_CLOCKRES_MIN != 20000000 )
                errx(1, "_POSIX_CLOCKRES_MIN is not 20000000");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
