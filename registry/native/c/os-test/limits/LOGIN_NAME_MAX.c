/* Test if LOGIN_NAME_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef LOGIN_NAME_MAX
        if ( LOGIN_NAME_MAX < _POSIX_LOGIN_NAME_MAX )
                errx(1, "LOGIN_NAME_MAX < _POSIX_LOGIN_NAME_MAX");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
