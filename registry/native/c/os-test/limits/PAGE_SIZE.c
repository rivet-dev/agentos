/*[XSI]*/
/* Test if PAGE_SIZE has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PAGE_SIZE
        if ( PAGE_SIZE < 1 )
                errx(1, "PAGE_SIZE < 1");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
