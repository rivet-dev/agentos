/*[XSI]*/
/* Test whether a basic endpwent invocation works. */

#include <errno.h>
#include <pwd.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t uid = getuid();
	errno = 0;
	setpwent();
	if ( errno )
		err(1, "setpwent");
	struct passwd* pwd;
	bool found = false;
	while ( (errno = 0, pwd = getpwent()) )
	{
		if ( pwd->pw_uid == uid )
			found = true;
	}
	if ( errno )
		err(1, "getpwent");
	if ( !found )
		errx(1, "did not find user");
	// Close the database.
	errno = 0;
	endpwent();
	if ( errno )
		err(1, "endpwent");
	found = false;
	// The database is not not open, and getpwent is required to reopen the
	// database and return the first entry. This will rewind the database.
	while ( (errno = 0, pwd = getpwent()) )
	{
		if ( pwd->pw_uid == uid )
			found = true;
	}
	if ( errno )
		err(1, "getpwent");
	if ( !found )
		errx(1, "did not find user again");
	return 0;
}
