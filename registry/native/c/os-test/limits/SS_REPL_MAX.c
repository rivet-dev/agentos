/*[SS|TSP]*/
/* Test if SS_REPL_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef SS_REPL_MAX
        if ( SS_REPL_MAX < _POSIX_SS_REPL_MAX )
                errx(1, "SS_REPL_MAX < _POSIX_SS_REPL_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
