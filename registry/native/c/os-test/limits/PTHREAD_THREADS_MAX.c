/* Test if PTHREAD_THREADS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PTHREAD_THREADS_MAX
        if ( PTHREAD_THREADS_MAX < _POSIX_THREAD_THREADS_MAX )
                errx(1, "PTHREAD_THREADS_MAX < _POSIX_THREAD_THREADS_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
