/* Test if _POSIX_THREAD_DESTRUCTOR_ITERATIONS has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_THREAD_DESTRUCTOR_ITERATIONS
        if ( _POSIX_THREAD_DESTRUCTOR_ITERATIONS != 4 )
                errx(1, "_POSIX_THREAD_DESTRUCTOR_ITERATIONS is not 4");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
