/* Test whether a basic strnlen invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	if ( strnlen("foo", 2) != 2 )
		errx(1, "strnlen did not return 2");
	return 0;
}
