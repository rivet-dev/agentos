/* Test whether a basic setegid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( setegid(getegid()) < 0 )
		err(1, "setegid");
	return 0;
}
