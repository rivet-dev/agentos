/* Test whether a basic getpwuid_r invocation works. */

#include <pwd.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t uid = getuid();
	long reasonable = sysconf(_SC_GETPW_R_SIZE_MAX);
	size_t size = 0 < reasonable ? reasonable : 64;
	char* buffer = malloc(size);
	if ( !buffer )
		err(1, "malloc");
	struct passwd entry;
	struct passwd* pwd;
	while ( (errno = getpwuid_r(uid, &entry, buffer, size, &pwd)) )
	{
		if ( errno == ERANGE )
		{
			size *= 2;
			if ( !(buffer = realloc(buffer, size)) )
				err(1, "malloc");
			continue;
		}
		err(1, "getpwuid_r");
	}
	if ( !pwd )
		errx(1, "user not found");
	if ( pwd->pw_uid != uid )
		errx(1, "wrong uid");
	return 0;
}
