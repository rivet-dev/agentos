/* Test if INT_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef INT_MIN
        if ( INT_MIN > -2147483648 )
                errx(1, "INT_MIN > -2147483648");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
