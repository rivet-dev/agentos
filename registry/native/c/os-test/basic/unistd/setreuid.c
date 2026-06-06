/*[XSI]*/
/* Test whether a basic setreuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t ruid = getuid();
	uid_t euid = geteuid();
	if ( setreuid(ruid, euid) < 0 )
		err(1, "setreuid");
	return 0;
}
