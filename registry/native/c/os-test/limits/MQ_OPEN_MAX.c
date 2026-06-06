/*[MSG]*/
/* Test if MQ_OPEN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef MQ_OPEN_MAX
        if ( MQ_OPEN_MAX < _POSIX_MQ_OPEN_MAX )
                errx(1, "MQ_OPEN_MAX < _POSIX_MQ_OPEN_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
