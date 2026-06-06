/* Test if STREAM_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef STREAM_MAX
        if ( STREAM_MAX < _POSIX_STREAM_MAX )
                errx(1, "STREAM_MAX < _POSIX_STREAM_MAX");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
