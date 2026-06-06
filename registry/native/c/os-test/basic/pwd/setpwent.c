/*[XSI]*/
/* Test whether a basic setpwent invocation works. */

#include <errno.h>
#include <pwd.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t uid = getuid();
	struct passwd* pwd;
	bool found = false;
	bool found_again = false;
	// getpwent is supposed to setpwent if needed.
	while ( (errno = 0, pwd = getpwent()) )
	{
		if ( pwd->pw_uid == uid )
		{
			if ( found )
			{
				found_again = true;
				break;
			}
			found = true;
			// Rewind the user database.
			errno = 0;
			setpwent();
			if ( errno )
				err(1, "setpwent");
		}
	}
	if ( errno )
		err(1, "getpwent");
	if ( !found )
		errx(1, "did not find user");
	if ( !found_again )
		errx(1, "did not find user again");
	return 0;
}
