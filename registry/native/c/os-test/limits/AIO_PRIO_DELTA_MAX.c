/* Test if AIO_PRIO_DELTA_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef AIO_PRIO_DELTA_MAX
        if ( AIO_PRIO_DELTA_MAX < 0 )
                errx(1, "AIO_PRIO_DELTA_MAX < 0");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
