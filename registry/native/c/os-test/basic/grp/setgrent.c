/*[XSI]*/
/* Test whether a basic setgrent invocation works. */

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
	bool found_again = false;
	// getgrent is supposed to setgrent if needed.
	while ( (errno = 0, grp = getgrent()) )
	{
		if ( grp->gr_gid == gid )
		{
			if ( found )
			{
				found_again = true;
				break;
			}
			found = true;
			// Rewind the group database.
			errno = 0;
			setgrent();
			if ( errno )
				err(1, "setgrent");
		}
	}
	if ( errno )
		err(1, "getgrent");
	if ( !found )
		errx(1, "did not find group");
	if ( !found_again )
		errx(1, "did not find group again");
	return 0;
}
