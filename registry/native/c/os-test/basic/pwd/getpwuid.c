/* Test whether a basic getpwuid invocation works. */

#include <pwd.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	uid_t uid = getuid();
	struct passwd* pwd = getpwuid(uid);
	if ( !pwd )
		err(1, "getpwuid");
	if ( pwd->pw_uid != uid )
		errx(1, "wrong uid");
	return 0;
}
