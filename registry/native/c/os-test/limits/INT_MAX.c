/* Test if INT_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef INT_MAX
        if ( INT_MAX < 2147483647 )
                errx(1, "INT_MAX < 2147483647");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
