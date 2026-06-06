/*[XSI]*/
/* Test whether a basic a64l invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long value = a64l("/.aZ");
	if ( value != 9854977 )
		errx(1, "a64l(\"/.aZ\") was %ld, not %ld", value, 9854977L);
	return 0;
}
