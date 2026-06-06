/* Test if TTY_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef TTY_NAME_MAX
        if ( TTY_NAME_MAX < _POSIX_TTY_NAME_MAX )
                errx(1, "TTY_NAME_MAX < _POSIX_TTY_NAME_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
