/*[MSG]*/
/* Test if _POSIX_MQ_PRIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef _POSIX_MQ_PRIO_MAX
        if ( _POSIX_MQ_PRIO_MAX != 32 )
                errx(1, "_POSIX_MQ_PRIO_MAX is not 32");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
