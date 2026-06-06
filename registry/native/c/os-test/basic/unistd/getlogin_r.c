/* Test whether a basic getlogin_r invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
#ifdef LOGIN_NAME_MAX
	char buf[LOGIN_NAME_MAX];
#elif defined(_POSIX_LOGIN_NAME_MAX)
	char buf[_POSIX_LOGIN_NAME_MAX];
#else
	char buf[9];
#endif
	int errnum = getlogin_r(buf, sizeof(buf));
	if ( errnum && errnum != ENOTTY && errnum != ENXIO )
	{
		errno = errnum;
		err(1, "getlogin_r");
	}
	return 0;
}
