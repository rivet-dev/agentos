/* Test whether a basic getpwnam invocation works. */

#include <pwd.h>
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
	pwd = getpwnam(user);
	if ( !pwd )
		err(1, "getpwnam");
	if ( pwd->pw_uid != uid )
		errx(1, "wrong uid");
	if ( strcmp(pwd->pw_name, user) != 0 )
		errx(1, "wrong name");
	return 0;
}
