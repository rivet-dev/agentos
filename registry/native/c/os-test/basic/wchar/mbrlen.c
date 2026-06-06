/* Test whether a basic mbrlen invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	size_t len = mbrlen("x", 1, &ps);
	if ( len != 1 )
		errx(1, "mbrlen did not return 1");
	return 0;
}
