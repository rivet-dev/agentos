/*[XSI]*/
/* Test whether a basic setresgid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t rgid, egid, sgid;
	if ( getresgid(&rgid, &egid, &sgid) < 0 )
		err(1, "getresgid");
	if ( setresgid(rgid, egid, sgid) < 0 )
		err(1, "setresgid");
	return 0;
}
