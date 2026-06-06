/* Test if _POSIX_PIPE_BUF has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_PIPE_BUF
        if ( _POSIX_PIPE_BUF != 512 )
                errx(1, "_POSIX_PIPE_BUF is not 512");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
