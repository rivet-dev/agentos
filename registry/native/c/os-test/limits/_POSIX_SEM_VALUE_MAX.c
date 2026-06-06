/* Test if _POSIX_SEM_VALUE_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_SEM_VALUE_MAX
        if ( _POSIX_SEM_VALUE_MAX != 32767 )
                errx(1, "_POSIX_SEM_VALUE_MAX is not 32767");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
