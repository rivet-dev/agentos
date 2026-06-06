/* Test if PIPE_BUF has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PIPE_BUF
        if ( PIPE_BUF < _POSIX_PIPE_BUF )
                errx(1, "PIPE_BUF < _POSIX_PIPE_BUF");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
