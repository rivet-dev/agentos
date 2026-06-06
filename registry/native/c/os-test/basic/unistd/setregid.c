/*[XSI]*/
/* Test whether a basic setregid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t rgid = getgid();
	gid_t egid = getegid();
	if ( setregid(rgid, egid) < 0 )
		err(1, "setregid");
	return 0;
}
