/*[SS|TSP]*/
/* Test if _POSIX_SS_REPL_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_SS_REPL_MAX
        if ( _POSIX_SS_REPL_MAX != 4 )
                errx(1, "_POSIX_SS_REPL_MAX is not 4");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
