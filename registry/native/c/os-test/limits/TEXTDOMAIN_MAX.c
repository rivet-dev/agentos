/* Test if TEXTDOMAIN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef TEXTDOMAIN_MAX
        if ( TEXTDOMAIN_MAX < _POSIX_TEXTDOMAIN_MAX - 3 )
                errx(1, "TEXTDOMAIN_MAX < _POSIX_TEXTDOMAIN_MAX - 3");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
