/* Test whether a basic gethostname invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
#ifdef HOST_NAME_MAX
	char buf[HOST_NAME_MAX];
#elif defined(_POSIX_HOST_NAME_MAX)
	char buf[_POSIX_HOST_NAME_MAX];
#else
	char buf[255];
#endif
	if ( gethostname(buf, sizeof(buf)) < 0 )
		err(1, "gethostname");
	return 0;
}
