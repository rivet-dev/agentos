/* Test if MB_LEN_MAX has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef MB_LEN_MAX
        if ( MB_LEN_MAX < 1 )
                errx(1, "MB_LEN_MAX < 1");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
