/* Test whether a basic getpwnam_r invocation works. */

#include <pwd.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t uid = getuid();
	struct passwd* pwd = getpwuid(uid);
	if ( !pwd )
		err(1, "getpwuid");
	char* user = strdup(pwd->pw_name);
	if ( !user )
		errx(1, "malloc");
	long reasonable = sysconf(_SC_GETPW_R_SIZE_MAX);
	size_t size = 0 < reasonable ? reasonable : 64;
	char* buffer = malloc(size);
	if ( !buffer )
		err(1, "malloc");
	struct passwd entry;
	while ( (errno = getpwnam_r(user, &entry, buffer, size, &pwd)) )
	{
		if ( errno == ERANGE )
		{
			size *= 2;
			if ( !(buffer = realloc(buffer, size)) )
				err(1, "malloc");
			continue;
		}
		err(1, "getpwnam_r");
	}
	if ( !pwd )
		errx(1, "user not found");
	if ( strcmp(pwd->pw_name, user) != 0 )
		errx(1, "wrong name");
	return 0;
}
