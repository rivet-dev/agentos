/* Test whether a basic sigsetjmp invocation works. */

#include <setjmp.h>

#include "../basic.h"

int main(void)
{
	sigjmp_buf buf;
	if ( sigsetjmp(buf, 0) )
		errx(1, "sigsetjmp(0) != 0");
	if ( sigsetjmp(buf, 1) )
		errx(1, "sigsetjmp(0) != 0");
	return 0;
}
