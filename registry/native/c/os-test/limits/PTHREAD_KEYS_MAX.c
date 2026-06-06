/* Test if PTHREAD_KEYS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PTHREAD_KEYS_MAX
        if ( PTHREAD_KEYS_MAX < _POSIX_THREAD_KEYS_MAX )
                errx(1, "PTHREAD_KEYS_MAX < _POSIX_THREAD_KEYS_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
