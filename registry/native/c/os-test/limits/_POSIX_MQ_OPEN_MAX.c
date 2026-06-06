/*[MSG]*/
/* Test if _POSIX_MQ_OPEN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_MQ_OPEN_MAX
        if ( _POSIX_MQ_OPEN_MAX != 8 )
                errx(1, "_POSIX_MQ_OPEN_MAX is not 8");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
