/* Test whether a basic mbsinit invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	if ( mbsinit(&ps) == 0 )
		errx(1, "mbsinit(&ps) == 0");
	return 0;
}
