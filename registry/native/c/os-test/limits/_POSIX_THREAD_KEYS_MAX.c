/* Test if _POSIX_THREAD_KEYS_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_THREAD_KEYS_MAX
        if ( _POSIX_THREAD_KEYS_MAX != 128 )
                errx(1, "_POSIX_THREAD_KEYS_MAX is not 128");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
