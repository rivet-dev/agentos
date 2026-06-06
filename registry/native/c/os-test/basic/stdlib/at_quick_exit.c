/* Test whether a basic at_quick_exit invocation works. */

#include <stdlib.h>

#include "../basic.h"

static void handler(void)
{
	_Exit(0);
}

int main(void)
{
	if ( at_quick_exit(handler) )
		errx(1, "at_quick_exit failed");
	quick_exit(1);
}
