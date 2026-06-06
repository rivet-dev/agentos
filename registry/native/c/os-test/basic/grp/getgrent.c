/*[XSI]*/
/* Test whether a basic getgrent invocation works. */

#include <errno.h>
#include <grp.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t gid = getgid();
	struct group* grp;
	bool found = false;
	// getgrent is supposed to setgrent if needed.
	while ( (errno = 0, grp = getgrent()) )
	{
		if ( grp->gr_gid == gid )
			found = true;
	}
	if ( errno )
		err(1, "getgrent");
	if ( !found )
		errx(1, "did not find group");
	return 0;
}
