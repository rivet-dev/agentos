/* Test whether a basic longjmp invocation works. */

#include <setjmp.h>
#include <stdbool.h>

#include "../basic.h"

int main(void)
{
	volatile bool done = false;
	jmp_buf buf;
	int ret = setjmp(buf);
	if ( ret )
	{
		if ( !done )
			errx(1, "setjmp returned early");
		if ( ret != 1 )
			errx(1, "setjmp() != 1");
		return 0;
	}
	if ( done )
		errx(1, "longjmp did not change 0 to 1");
	done = true;
	longjmp(buf, 0);
	return 1;
}
