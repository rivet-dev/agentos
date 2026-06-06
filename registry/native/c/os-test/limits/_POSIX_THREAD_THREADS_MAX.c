/* Test if _POSIX_THREAD_THREADS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_THREAD_THREADS_MAX
        if ( _POSIX_THREAD_THREADS_MAX != 64 )
                errx(1, "_POSIX_THREAD_THREADS_MAX is not 64");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
