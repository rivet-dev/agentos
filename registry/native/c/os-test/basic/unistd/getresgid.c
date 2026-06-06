/*[XSI]*/
/* Test whether a basic getresgid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t rgid, egid, sgid;
	if ( getresgid(&rgid, &egid, &sgid) < 0 )
		err(1, "getresgid");
	return 0;
}
