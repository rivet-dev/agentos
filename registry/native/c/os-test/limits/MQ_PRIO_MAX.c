/*[MSG]*/
/* Test if MQ_PRIO_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef MQ_PRIO_MAX
        if ( MQ_PRIO_MAX < _POSIX_MQ_PRIO_MAX )
                errx(1, "MQ_PRIO_MAX < _POSIX_MQ_PRIO_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
