/*[XSI]*/
/* Test whether a basic getresuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t ruid, euid, suid;
	if ( getresuid(&ruid, &euid, &suid) < 0 )
		err(1, "getresuid");
	return 0;
}
