/* Test if MAX_INPUT has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef MAX_INPUT
        if ( MAX_INPUT < _POSIX_MAX_INPUT )
                errx(1, "MAX_INPUT < _POSIX_MAX_INPUT");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
