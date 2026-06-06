/* Test whether a basic strlen invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	if ( strlen("foo") != 3 )
		errx(1, "strlen did not return 3");
	return 0;
}
