/*[XSI]*/
/* Test whether a basic getutxline invocation works. */

#include <errno.h>
#include <utmpx.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	setutxent();
	if ( errno )
		err(1, "setutxent");

	errno = 0;
	struct utmpx in = { .ut_line = "os-test" };
	struct utmpx* data = getutxline(&in);
	// de facto: It's unclear to me why this call sets EACCES on many systems,
	// while the other getter entries in utmp does not. However, I'm not going
	// to punish the implementation for enforcing security. I do suspect it may
	// be an errno issue internally in the function, so do check for such a bug.
	if ( !data && errno && errno != EACCES )
		err(1, "getutxline");
	return 0;
}
