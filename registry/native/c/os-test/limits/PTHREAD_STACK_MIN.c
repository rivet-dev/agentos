/* Test if PTHREAD_STACK_MIN has the correct value. */

#include "suite.h"

int main(void)
{
#ifdef PTHREAD_STACK_MIN
        if ( PTHREAD_STACK_MIN < 0 )
                errx(1, "PTHREAD_STACK_MIN < 0");
        return 0;
#else
        errx(1, "missing_optional");
#endif
}
