/* Test if WORD_BIT has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef WORD_BIT
        if ( WORD_BIT < 32 )
                errx(1, "WORD_BIT < 32");
        return 0;
#else
        errx(1, "undeclared");
#endif
}
