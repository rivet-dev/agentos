/* Test whether a basic setjmp invocation works. */

#include <setjmp.h>

#include "../basic.h"

int main(void)
{
	jmp_buf buf;
	if ( setjmp(buf) )
		errx(1, "setjmp() != 0");
	return 0;
}
