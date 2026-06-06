/*[XSI]*/
/* Test whether a basic endgrent invocation works. */

#include <errno.h>
#include <grp.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t gid = getgid();
	errno = 0;
	setgrent();
	if ( errno )
		err(1, "setgrent");
	struct group* grp;
	bool found = false;
	while ( (errno = 0, grp = getgrent()) )
	{
		if ( grp->gr_gid == gid )
			found = true;
	}
	if ( errno )
		err(1, "getgrent");
	if ( !found )
		errx(1, "did not find group");
	// Close the database.
	errno = 0;
	endgrent();
	if ( errno )
		err(1, "endgrent");
	found = false;
	// The database is not not open, and getgrent is required to reopen the
	// database and return the first entry. This will rewind the database.
	while ( (errno = 0, grp = getgrent()) )
	{
		if ( grp->gr_gid == gid )
			found = true;
	}
	if ( errno )
		err(1, "getgrent");
	if ( !found )
		errx(1, "did not find group again");
	return 0;
}
