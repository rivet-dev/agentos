/* Test whether a basic sig2str invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	char name[SIG2STR_MAX];
	if ( sig2str(SIGUSR1, name) < 0 )
		err(1, "sig2str");
	const char* expected = "USR1";
	if ( strcmp(name, expected) != 0 )
		errx(1, "sig2str gave %s not %s", name, expected);
	return 0;
}
