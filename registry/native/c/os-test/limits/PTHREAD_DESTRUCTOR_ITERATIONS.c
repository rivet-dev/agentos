/* Test if PTHREAD_DESTRUCTOR_ITERATIONS has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PTHREAD_DESTRUCTOR_ITERATIONS
        if ( PTHREAD_DESTRUCTOR_ITERATIONS < _POSIX_THREAD_DESTRUCTOR_ITERATIONS )
                errx(1, "PTHREAD_DESTRUCTOR_ITERATIONS < _POSIX_THREAD_DESTRUCTOR_ITERATIONS");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
