/* Test if RE_DUP_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef RE_DUP_MAX
        if ( RE_DUP_MAX < _POSIX_RE_DUP_MAX )
                errx(1, "RE_DUP_MAX < _POSIX_RE_DUP_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
