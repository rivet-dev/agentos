/* Test whether a basic getlogin invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( !getlogin() && errno != ENOTTY && errno != ENXIO )
		err(1, "getlogin");
	return 0;
}
